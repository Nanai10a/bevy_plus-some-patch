use bevy::prelude::*;
use rand::{rngs::StdRng, Rng, SeedableRng};

fn main() {
    AppBuilder::new()
        .add_defaults()
        .add_system(build_move_system())
        .add_system(bevy::diagnostics::build_fps_printer_system())
        .setup_world(setup)
        .run();
}

fn build_move_system() -> Box<dyn Schedulable> {
    SystemBuilder::new("Move")
        .read_resource::<Time>()
        .with_query(<(Write<Translation>, Write<StandardMaterial>)>::query())
        .build(move |_, world, time, person_query| {
            for (mut translation, mut material) in person_query.iter_mut(world) {
                translation.0 += math::vec3(1.0, 0.0, 0.0) * time.delta_seconds;
                if let ColorSource::Color(color) = material.albedo {
                    material.albedo = (color
                        + Color::rgb(-time.delta_seconds, -time.delta_seconds, time.delta_seconds))
                    .into();
                }
            }
        })
}

fn setup(world: &mut World, resources: &mut Resources) {
    let mut mesh_storage = resources.get_mut::<AssetStorage<Mesh>>().unwrap();
    let cube_handle = mesh_storage.add(Mesh::load(MeshType::Cube));
    let plane_handle = mesh_storage.add(Mesh::load(MeshType::Plane { size: 10.0 }));

    let mut builder = world
        .build()
        // plane
        .add_entity(MeshEntity {
            mesh: plane_handle,
            material: StandardMaterial {
                albedo: Color::rgb(0.1, 0.2, 0.1).into(),
            },
            ..Default::default()
        })
        // cube
        .add_entity(MeshEntity {
            mesh: cube_handle,
            material: StandardMaterial {
                albedo: Color::rgb(1.0, 1.0, 1.0).into(),
            },
            translation: Translation::new(0.0, 0.0, 1.0),
            ..Default::default()
        })
        .add_entity(MeshEntity {
            mesh: cube_handle,
            material: StandardMaterial {
                albedo: Color::rgb(0.0, 1.0, 0.0).into(),
            },
            translation: Translation::new(-2.0, 0.0, 1.0),
            ..Default::default()
        })
        // light
        .add_entity(LightEntity {
            translation: Translation::new(4.0, -4.0, 5.0),
            ..Default::default()
        })
        // camera
        .add_entity(CameraEntity {
            camera: Camera::new(CameraType::Projection {
                fov: std::f32::consts::PI / 4.0,
                near: 1.0,
                far: 1000.0,
                aspect_ratio: 1.0,
            }),
            active_camera: ActiveCamera,
            local_to_world: LocalToWorld(Mat4::look_at_rh(
                Vec3::new(3.0, 8.0, 5.0),
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            )),
        });

    let mut rng = StdRng::from_entropy();
    for _ in 0..10000 {
        builder = builder.add_entity(MeshEntity {
            mesh: cube_handle,
            material: StandardMaterial {
                albedo: Color::rgb(
                    rng.gen_range(0.0, 1.0),
                    rng.gen_range(0.0, 1.0),
                    rng.gen_range(0.0, 1.0),
                )
                .into(),
            },
            translation: Translation::new(
                rng.gen_range(-50.0, 50.0),
                rng.gen_range(-50.0, 50.0),
                0.0,
            ),
            ..Default::default()
        })
    }

    builder.build();
}
