mod converters;
mod winit_config;
mod winit_windows;

use std::{
    path::PathBuf,
    sync::{mpsc, Mutex},
    thread,
};

use bevy_input::{
    keyboard::KeyboardInput,
    mouse::{MouseButtonInput, MouseMotion, MouseScrollUnit, MouseWheel},
    touch::TouchInput,
};
pub use winit_config::*;
pub use winit_windows::*;

use bevy_app::{App, AppBuilder, AppExit, CoreStage, Events, ManualEventReader, Plugin};
use bevy_ecs::{system::IntoExclusiveSystem, world::World};
use bevy_math::{ivec2, Vec2};
use bevy_utils::tracing::{error, trace, warn};
use bevy_window::{
    CreateWindow, CursorEntered, CursorLeft, CursorMoved, FileDragAndDrop, ReceivedCharacter,
    WindowBackendScaleFactorChanged, WindowCloseRequested, WindowCreated, WindowFocused,
    WindowMoved, WindowResized, WindowScaleFactorChanged, Windows,
};
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event::{self, DeviceEvent, Event, Touch, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopProxy, EventLoopWindowTarget},
    window::WindowId,
};

use winit::dpi::LogicalSize;
#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
use winit::platform::unix::EventLoopExtUnix;

#[derive(Default)]
pub struct WinitPlugin;

impl Plugin for WinitPlugin {
    fn build(&self, app: &mut AppBuilder) {
        app.init_resource::<WinitWindows>()
            .set_runner(winit_runner_any_thread)
            .add_system_to_stage(CoreStage::PostUpdate, change_window.exclusive_system());
    }
}

fn change_window(world: &mut World) {
    let world = world.cell();
    let winit_windows = world.get_resource::<WinitWindows>().unwrap();
    let mut windows = world.get_resource_mut::<Windows>().unwrap();

    for bevy_window in windows.iter_mut() {
        let id = bevy_window.id();
        for command in bevy_window.drain_commands() {
            match command {
                bevy_window::WindowCommand::SetWindowMode {
                    mode,
                    resolution: (width, height),
                } => {
                    let window = winit_windows.get_window(id).unwrap();
                    match mode {
                        bevy_window::WindowMode::BorderlessFullscreen => {
                            window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)))
                        }
                        bevy_window::WindowMode::Fullscreen { use_size } => window.set_fullscreen(
                            Some(winit::window::Fullscreen::Exclusive(match use_size {
                                true => get_fitting_videomode(
                                    &window.current_monitor().unwrap(),
                                    width,
                                    height,
                                ),
                                false => get_best_videomode(&window.current_monitor().unwrap()),
                            })),
                        ),
                        bevy_window::WindowMode::Windowed => window.set_fullscreen(None),
                    }
                }
                bevy_window::WindowCommand::SetTitle { title } => {
                    let window = winit_windows.get_window(id).unwrap();
                    window.set_title(&title);
                }
                bevy_window::WindowCommand::SetScaleFactor { scale_factor } => {
                    let mut window_dpi_changed_events = world
                        .get_resource_mut::<Events<WindowScaleFactorChanged>>()
                        .unwrap();
                    window_dpi_changed_events.send(WindowScaleFactorChanged { id, scale_factor });
                }
                bevy_window::WindowCommand::SetResolution {
                    logical_resolution: (width, height),
                    scale_factor,
                } => {
                    let window = winit_windows.get_window(id).unwrap();
                    window.set_inner_size(
                        winit::dpi::LogicalSize::new(width, height)
                            .to_physical::<f64>(scale_factor),
                    );
                }
                bevy_window::WindowCommand::SetVsync { .. } => (),
                bevy_window::WindowCommand::SetResizable { resizable } => {
                    let window = winit_windows.get_window(id).unwrap();
                    window.set_resizable(resizable);
                }
                bevy_window::WindowCommand::SetDecorations { decorations } => {
                    let window = winit_windows.get_window(id).unwrap();
                    window.set_decorations(decorations);
                }
                bevy_window::WindowCommand::SetCursorLockMode { locked } => {
                    let window = winit_windows.get_window(id).unwrap();
                    window
                        .set_cursor_grab(locked)
                        .unwrap_or_else(|e| error!("Unable to un/grab cursor: {}", e));
                }
                bevy_window::WindowCommand::SetCursorVisibility { visible } => {
                    let window = winit_windows.get_window(id).unwrap();
                    window.set_cursor_visible(visible);
                }
                bevy_window::WindowCommand::SetCursorPosition { position } => {
                    let window = winit_windows.get_window(id).unwrap();
                    let inner_size = window.inner_size().to_logical::<f32>(window.scale_factor());
                    window
                        .set_cursor_position(winit::dpi::LogicalPosition::new(
                            position.x,
                            inner_size.height - position.y,
                        ))
                        .unwrap_or_else(|e| error!("Unable to set cursor position: {}", e));
                }
                bevy_window::WindowCommand::SetMaximized { maximized } => {
                    let window = winit_windows.get_window(id).unwrap();
                    window.set_maximized(maximized)
                }
                bevy_window::WindowCommand::SetMinimized { minimized } => {
                    let window = winit_windows.get_window(id).unwrap();
                    window.set_minimized(minimized)
                }
                bevy_window::WindowCommand::SetPosition { position } => {
                    let window = winit_windows.get_window(id).unwrap();
                    window.set_outer_position(PhysicalPosition {
                        x: position[0],
                        y: position[1],
                    });
                }
                bevy_window::WindowCommand::SetResizeConstraints { resize_constraints } => {
                    let window = winit_windows.get_window(id).unwrap();
                    let constraints = resize_constraints.check_constraints();
                    let min_inner_size = LogicalSize {
                        width: constraints.min_width,
                        height: constraints.min_height,
                    };
                    let max_inner_size = LogicalSize {
                        width: constraints.max_width,
                        height: constraints.max_height,
                    };

                    window.set_min_inner_size(Some(min_inner_size));
                    if constraints.max_width.is_finite() && constraints.max_height.is_finite() {
                        window.set_max_inner_size(Some(max_inner_size));
                    }
                }
            }
        }
    }
}

fn run<F>(event_loop: EventLoop<()>, event_handler: F) -> !
where
    F: 'static + FnMut(Event<'_, ()>, &EventLoopWindowTarget<()>, &mut ControlFlow),
{
    event_loop.run(event_handler)
}

// TODO: It may be worth moving this cfg into a procedural macro so that it can be referenced by
// a single name instead of being copied around.
// https://gist.github.com/jakerr/231dee4a138f7a5f25148ea8f39b382e seems to work.
#[cfg(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn run_return<F>(event_loop: &mut EventLoop<()>, event_handler: F)
where
    F: FnMut(Event<'_, ()>, &EventLoopWindowTarget<()>, &mut ControlFlow),
{
    use winit::platform::run_return::EventLoopExtRunReturn;
    event_loop.run_return(event_handler)
}

#[cfg(not(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
)))]
fn run_return<F>(_event_loop: &mut EventLoop<()>, _event_handler: F)
where
    F: FnMut(Event<'_, ()>, &EventLoopWindowTarget<()>, &mut ControlFlow),
{
    panic!("Run return is not supported on this platform!")
}

pub fn winit_runner(app: App) {
    winit_runner_with(app, false);
}

#[cfg(any(
    target_os = "linux",
    target_os = "dragonfly",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
pub fn winit_runner_any_thread(app: App) {
    winit_runner_with(app, true);
}

pub fn winit_runner_with(mut app: App, is_any_thread: bool) {
    if !is_any_thread {
        panic!("non-any-thread is not supported!");
    }

    let should_return_from_run = app
        .world
        .get_resource::<WinitConfig>()
        .map_or(false, |config| config.return_from_run);

    let (app_exit_event_sender, app_exit_event_receiver) = mpsc::sync_channel::<()>(0);
    let (winit_event_sender, winit_event_receiver) = mpsc::channel::<WinitEvent>();

    let (keyboard_input_sender, keyboard_input_receiver) = mpsc::channel::<KeyboardInput>();
    app.world
        .insert_resource(Mutex::new(keyboard_input_receiver));

    thread::spawn(move || {
        let mut event_loop = EventLoop::new_any_thread();
        winit_event_sender
            .send(WinitEvent::CreatedProxy(event_loop.create_proxy()))
            .unwrap();

        trace!("Entering winit event loop");

        let event_handler = move |event: Event<()>,
                                  event_loop: &EventLoopWindowTarget<()>,
                                  control_flow: &mut ControlFlow| {
            *control_flow = ControlFlow::Poll;

            if let Ok(_) = app_exit_event_receiver.try_recv() {
                *control_flow = ControlFlow::Exit;
            }

            let e = match event {
                event::Event::WindowEvent {
                    event,
                    window_id: winit_window_id,
                    ..
                } => {
                    let e = match event {
                        WindowEvent::Resized(size) => WinitWindowEvent::Resized(size),
                        WindowEvent::CloseRequested => WinitWindowEvent::CloseRequested,
                        WindowEvent::KeyboardInput { ref input, .. } => {
                            let input = converters::convert_keyboard_input(input);

                            keyboard_input_sender.send(input.clone()).unwrap();

                            WinitWindowEvent::KeyboardInput(input)
                        }
                        WindowEvent::CursorMoved { position, .. } => {
                            WinitWindowEvent::CursorMoved(position)
                        }
                        WindowEvent::CursorEntered { .. } => WinitWindowEvent::CursorEntered,
                        WindowEvent::CursorLeft { .. } => WinitWindowEvent::CursorLeft,
                        WindowEvent::MouseInput { state, button, .. } => {
                            WinitWindowEvent::MouseInput(MouseButtonInput {
                                button: converters::convert_mouse_button(button),
                                state: converters::convert_element_state(state),
                            })
                        }
                        WindowEvent::MouseWheel { delta, .. } => match delta {
                            event::MouseScrollDelta::LineDelta(x, y) => {
                                WinitWindowEvent::MouseWheel(MouseWheel {
                                    unit: MouseScrollUnit::Line,
                                    x,
                                    y,
                                })
                            }
                            event::MouseScrollDelta::PixelDelta(p) => {
                                WinitWindowEvent::MouseWheel(MouseWheel {
                                    unit: MouseScrollUnit::Pixel,
                                    x: p.x as f32,
                                    y: p.y as f32,
                                })
                            }
                        },
                        WindowEvent::Touch(touch) => WinitWindowEvent::Touch(touch),
                        WindowEvent::ReceivedCharacter(c) => WinitWindowEvent::ReceivedCharacter(c),
                        WindowEvent::ScaleFactorChanged {
                            scale_factor,
                            new_inner_size,
                        } => WinitWindowEvent::ScaleFactorChanged(
                            scale_factor,
                            new_inner_size.clone(),
                        ),
                        WindowEvent::Focused(focused) => WinitWindowEvent::Focused(focused),
                        WindowEvent::DroppedFile(path_buf) => {
                            WinitWindowEvent::DroppedFile(path_buf)
                        }
                        WindowEvent::HoveredFile(path_buf) => {
                            WinitWindowEvent::HoveredFile(path_buf)
                        }
                        WindowEvent::HoveredFileCancelled => WinitWindowEvent::HoveredFileCancelled,
                        WindowEvent::Moved(position) => WinitWindowEvent::Moved(position),
                        _ => WinitWindowEvent::None,
                    };

                    WinitEvent::WindowEvent(e, winit_window_id)
                }
                event::Event::DeviceEvent {
                    event: DeviceEvent::MouseMotion { delta },
                    ..
                } => WinitEvent::MouseMotion(MouseMotion {
                    delta: Vec2::new(delta.0 as f32, delta.1 as f32),
                }),
                event::Event::MainEventsCleared => WinitEvent::MainEventsCleared(
                    event_loop as *const EventLoopWindowTarget<()> as usize,
                ),
                _ => WinitEvent::None,
            };

            winit_event_sender.send(e).unwrap();
        };

        if should_return_from_run {
            run_return(&mut event_loop, event_handler);
        } else {
            run(event_loop, event_handler);
        }
    });

    let mut create_window_event_reader = ManualEventReader::<CreateWindow>::default();
    let mut app_exit_event_reader = ManualEventReader::<AppExit>::default();

    let mut current_elwt = None;

    trace!("Entering bevy (from winit) event loop");

    loop {
        if let Some(app_exit_events) = app.world.get_resource_mut::<Events<AppExit>>() {
            if app_exit_event_reader
                .iter(&app_exit_events)
                .next_back()
                .is_some()
            {
                app_exit_event_sender.send(()).unwrap();
            }
        }

        let mut drainer = vec![]; // FIXME: Smallvec化 + channelをsyncにして容量の制限
        winit_event_receiver
            .try_iter()
            .for_each(|e| drainer.push(e));

        for e in drainer.drain(..) {
            match e {
                WinitEvent::WindowEvent(e, winit_window_id) => {
                    let world = app.world.cell();
                    let winit_windows = world.get_resource_mut::<WinitWindows>().unwrap();
                    let mut windows = world.get_resource_mut::<Windows>().unwrap();
                    let window_id =
                        if let Some(window_id) = winit_windows.get_window_id(winit_window_id) {
                            window_id
                        } else {
                            warn!(
                                "Skipped event for unknown winit Window Id {:?}",
                                winit_window_id
                            );
                            return;
                        };

                    let window = if let Some(window) = windows.get_mut(window_id) {
                        window
                    } else {
                        warn!("Skipped event for unknown Window Id {:?}", winit_window_id);
                        return;
                    };

                    match e {
                        WinitWindowEvent::Resized(size) => {
                            window.update_actual_size_from_backend(size.width, size.height);
                            let mut resize_events =
                                world.get_resource_mut::<Events<WindowResized>>().unwrap();
                            resize_events.send(WindowResized {
                                id: window_id,
                                width: window.width(),
                                height: window.height(),
                            });
                        }
                        WinitWindowEvent::CloseRequested => world
                            .get_resource_mut::<Events<WindowCloseRequested>>()
                            .unwrap()
                            .send(WindowCloseRequested { id: window_id }),
                        WinitWindowEvent::KeyboardInput(input) => world
                            .get_resource_mut::<Events<KeyboardInput>>()
                            .unwrap()
                            .send(input),
                        WinitWindowEvent::CursorMoved(position) => {
                            let mut cursor_moved_events =
                                world.get_resource_mut::<Events<CursorMoved>>().unwrap();
                            let winit_window = winit_windows.get_window(window_id).unwrap();
                            let position = position.to_logical(winit_window.scale_factor());
                            let inner_size = winit_window
                                .inner_size()
                                .to_logical::<f32>(winit_window.scale_factor());

                            // move origin to bottom left
                            let y_position = inner_size.height - position.y;

                            let position = Vec2::new(position.x, y_position);
                            window.update_cursor_position_from_backend(Some(position));

                            cursor_moved_events.send(CursorMoved {
                                id: window_id,
                                position,
                            });
                        }
                        WinitWindowEvent::CursorEntered => world
                            .get_resource_mut::<Events<CursorEntered>>()
                            .unwrap()
                            .send(CursorEntered { id: window_id }),
                        WinitWindowEvent::CursorLeft => world
                            .get_resource_mut::<Events<CursorLeft>>()
                            .unwrap()
                            .send(CursorLeft { id: window_id }),
                        WinitWindowEvent::MouseInput(input) => world
                            .get_resource_mut::<Events<MouseButtonInput>>()
                            .unwrap()
                            .send(input),
                        WinitWindowEvent::MouseWheel(input) => world
                            .get_resource_mut::<Events<MouseWheel>>()
                            .unwrap()
                            .send(input),
                        WinitWindowEvent::Touch(touch) => {
                            let mut touch_input_events =
                                world.get_resource_mut::<Events<TouchInput>>().unwrap();

                            let winit_window = winit_windows.get_window(window_id).unwrap();
                            let mut location =
                                touch.location.to_logical(winit_window.scale_factor());

                            // On a mobile window, the start is from the top while on PC/Linux/OSX from
                            // bottom
                            if cfg!(target_os = "android") || cfg!(target_os = "ios") {
                                let window_height = windows.get_primary().unwrap().height();
                                location.y = window_height - location.y;
                            }
                            touch_input_events
                                .send(converters::convert_touch_input(touch, location));
                        }
                        WinitWindowEvent::ReceivedCharacter(c) => {
                            let mut char_input_events = world
                                .get_resource_mut::<Events<ReceivedCharacter>>()
                                .unwrap();

                            char_input_events.send(ReceivedCharacter {
                                id: window_id,
                                char: c,
                            });
                        }
                        WinitWindowEvent::ScaleFactorChanged(scale_factor, new_inner_size) => {
                            let mut backend_scale_factor_change_events = world
                                .get_resource_mut::<Events<WindowBackendScaleFactorChanged>>()
                                .unwrap();
                            backend_scale_factor_change_events.send(
                                WindowBackendScaleFactorChanged {
                                    id: window_id,
                                    scale_factor,
                                },
                            );

                            #[allow(clippy::float_cmp)]
                            if window.scale_factor() != scale_factor {
                                let mut scale_factor_change_events = world
                                    .get_resource_mut::<Events<WindowScaleFactorChanged>>()
                                    .unwrap();

                                scale_factor_change_events.send(WindowScaleFactorChanged {
                                    id: window_id,
                                    scale_factor,
                                });
                            }

                            window.update_scale_factor_from_backend(scale_factor);

                            if window.physical_width() != new_inner_size.width
                                || window.physical_height() != new_inner_size.height
                            {
                                let mut resize_events =
                                    world.get_resource_mut::<Events<WindowResized>>().unwrap();
                                resize_events.send(WindowResized {
                                    id: window_id,
                                    width: window.width(),
                                    height: window.height(),
                                });
                            }
                            window.update_actual_size_from_backend(
                                new_inner_size.width,
                                new_inner_size.height,
                            );
                        }
                        WinitWindowEvent::Focused(focused) => {
                            window.update_focused_status_from_backend(focused);
                            let mut focused_events =
                                world.get_resource_mut::<Events<WindowFocused>>().unwrap();
                            focused_events.send(WindowFocused {
                                id: window_id,
                                focused,
                            });
                        }
                        WinitWindowEvent::DroppedFile(path_buf) => {
                            let mut events =
                                world.get_resource_mut::<Events<FileDragAndDrop>>().unwrap();
                            events.send(FileDragAndDrop::DroppedFile {
                                id: window_id,
                                path_buf,
                            });
                        }
                        WinitWindowEvent::HoveredFile(path_buf) => {
                            let mut events =
                                world.get_resource_mut::<Events<FileDragAndDrop>>().unwrap();
                            events.send(FileDragAndDrop::HoveredFile {
                                id: window_id,
                                path_buf,
                            });
                        }
                        WinitWindowEvent::HoveredFileCancelled => {
                            let mut events =
                                world.get_resource_mut::<Events<FileDragAndDrop>>().unwrap();
                            events.send(FileDragAndDrop::HoveredFileCancelled { id: window_id });
                        }
                        WinitWindowEvent::Moved(position) => {
                            let position = ivec2(position.x, position.y);
                            window.update_actual_position_from_backend(position);
                            let mut events =
                                world.get_resource_mut::<Events<WindowMoved>>().unwrap();
                            events.send(WindowMoved {
                                id: window_id,
                                position,
                            });
                        }
                        WinitWindowEvent::None => (),
                    }
                }
                WinitEvent::MouseMotion(input) => {
                    let mut mouse_motion_events =
                        app.world.get_resource_mut::<Events<MouseMotion>>().unwrap();
                    mouse_motion_events.send(input);
                }
                WinitEvent::CreatedProxy(proxy) => app.world.insert_non_send(proxy),

                WinitEvent::MainEventsCleared(raw_elwt_ptr) => {
                    current_elwt = Some(unsafe {
                        (raw_elwt_ptr as *const EventLoopWindowTarget<()>)
                            .as_ref()
                            .unwrap()
                    });
                }
                WinitEvent::None => (),
            }
        }

        if let Some(elwt) = current_elwt {
            handle_create_window_events(&mut app.world, elwt, &mut create_window_event_reader);
            app.update();
        }
    }
}

fn handle_create_window_events(
    world: &mut World,
    event_loop: &EventLoopWindowTarget<()>,
    create_window_event_reader: &mut ManualEventReader<CreateWindow>,
) {
    let world = world.cell();
    let mut winit_windows = world.get_resource_mut::<WinitWindows>().unwrap();
    let mut windows = world.get_resource_mut::<Windows>().unwrap();
    let create_window_events = world.get_resource::<Events<CreateWindow>>().unwrap();
    let mut window_created_events = world.get_resource_mut::<Events<WindowCreated>>().unwrap();
    for create_window_event in create_window_event_reader.iter(&create_window_events) {
        let window = winit_windows.create_window(
            event_loop,
            create_window_event.id,
            &create_window_event.descriptor,
        );
        windows.add(window);
        window_created_events.send(WindowCreated {
            id: create_window_event.id,
        });
    }
}

enum WinitEvent {
    WindowEvent(WinitWindowEvent, WindowId),
    MouseMotion(MouseMotion),
    MainEventsCleared(usize),
    CreatedProxy(EventLoopProxy<()>),
    None,
}

enum WinitWindowEvent {
    Resized(PhysicalSize<u32>),
    CloseRequested,
    KeyboardInput(KeyboardInput),
    CursorMoved(PhysicalPosition<f64>),
    CursorEntered,
    CursorLeft,
    MouseInput(MouseButtonInput),
    MouseWheel(MouseWheel),
    Touch(Touch),
    ReceivedCharacter(char),
    ScaleFactorChanged(f64, PhysicalSize<u32>),
    Focused(bool),
    DroppedFile(PathBuf),
    HoveredFile(PathBuf),
    HoveredFileCancelled,
    Moved(PhysicalPosition<i32>),
    None,
}
