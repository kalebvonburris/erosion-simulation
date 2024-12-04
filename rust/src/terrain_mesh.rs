use godot::classes::{ImageTexture, RenderingServer};
use godot::{
    classes::{
        image::Format, CompressedTexture2D, IMeshInstance3D, Image, MeshInstance3D, ResourceLoader,
        Shader, ShaderMaterial,
    },
    obj::NewGd,
    prelude::*,
};
use rayon::prelude::*;

use std::sync::mpsc::{channel, Sender};
use std::sync::RwLock;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use lazy_static::lazy_static;

use crate::create_raindrops;
use crate::raindrop::Raindrop;

lazy_static! {
    static ref IMAGE_ID: RwLock<Rid> = RwLock::new(Rid::new(0));
    static ref DIMS: RwLock<(usize, usize)> = RwLock::new((0, 0));
    static ref TEXTURE: RwLock<Vec<f16>> = RwLock::new(Vec::with_capacity(0));
    static ref THREAD: Mutex<Option<(std::thread::JoinHandle<()>, Sender<()>)>> = Mutex::new(None);
    static ref MOUSE_POS: RwLock<Vector2> = RwLock::new(Vector2::new(0.0, 0.0));
    static ref DRAGGING: RwLock<bool> = RwLock::new(false);
}

#[derive(GodotClass)]
#[class(base=MeshInstance3D)]
struct TerrainMesh {
    base: Base<MeshInstance3D>,
    #[var]
    gravity: f32,
    /// Carrying capcity of the `Raindrop` - how much sediment it can carry.
    #[var]
    capacity: f32,
    #[var]
    inertia: f32,
    #[var]
    erosion_factor: f32,
    #[var]
    deposition_factor: f32,
    /// The diameter of the `Raindrop` - how much area it covers.
    /// This should almost always be >= 3.0, otherwise we get weird
    /// artifacts and terrible simulation.
    #[var]
    diameter: f32,
    #[var]
    lifetime: u32,
    #[var]
    starting_mass: f32,
}

#[godot_api]
impl IMeshInstance3D for TerrainMesh {
    fn init(base: Base<MeshInstance3D>) -> Self {
        godot_print!("Hello, world!"); // Prints to the Godot console
        Self { 
            base,
            gravity: 10.0,
            capacity: 2.0,
            inertia: 0.3,
            erosion_factor: 0.3,
            deposition_factor: 0.3,
            diameter: 3.0,
            lifetime: 50,
            starting_mass: 1.0,
        }
    }

    fn ready(&mut self) {
        // Get base terrain texture resource
        let resource = ResourceLoader::singleton()
            .load("res://terrain_texture.exr")
            .expect("terrain_texture.exr not found");

        // Try to cast the resource to a CompressedTexture2D
        match resource.try_cast::<CompressedTexture2D>() {
            Ok(base_texture) => {
                // Get the dimensions of the texture
                let image = base_texture.get_image().unwrap();
                let x = image.get_width();
                let y = image.get_height();

                // Set the dimensions of the texture
                *DIMS.write().unwrap() = (x as usize, y as usize);

                godot_print!("Texture dimensions: ({}, {})", x, y);

                // Get data for the texture
                let data = image.get_data().to_vec();

                godot_print!("{:?}", data.len());

                let mut converted: Vec<f16> = data
                    .chunks_exact(2)
                    .map(TryInto::try_into)
                    .map(Result::unwrap)
                    .map(f16::from_le_bytes)
                    .collect();

                let image_format = base_texture.get_image().unwrap().get_format();

                let bytes_to_skip = match image_format {
                    Format::RGH => 2,
                    Format::RGBH => 3,
                    Format::RGBAH => 4,
                    _ => {
                        godot_error!("Unsupported image format: {:?}", image_format);
                        return;
                    }
                };

                // Grab only every 3rd element
                converted = converted
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| i % bytes_to_skip == 0)
                    .map(|(_, x)| *x)
                    .collect();

                // Put the data into the static texture
                let mut texture_lock = TEXTURE.write().unwrap();
                texture_lock.clear();
                texture_lock.extend(converted.clone());
                godot_print!("{:?}", texture_lock.len());

                // Create BytePackedArray
                let mut array = PackedByteArray::new();
                array.extend(
                    converted
                        .iter()
                        .flat_map(|&x| x.to_le_bytes())
                        .collect::<Vec<u8>>(),
                );

                // Create a new texture
                let new_image = Image::create_from_data(x, y, false, Format::RH, &array).unwrap();

                let new_texture = ImageTexture::create_from_image(&new_image).unwrap();

                *IMAGE_ID.write().unwrap() = new_texture.get_rid();

                // Create a new ShaderMaterial
                let mut material = ShaderMaterial::new_gd();
                material.set_shader_parameter("terrain_texture", &new_texture.to_variant());

                // Load the height shader
                let shader_resource = ResourceLoader::singleton()
                    .load("res://height_shader.gdshader")
                    .expect("height_shader.gdshader not found");

                // Try to cast the resource to a Shader
                let shader = shader_resource.cast::<Shader>();
                material.set_shader(&shader);

                // Set the ShaderMaterial on the mesh
                self.base_mut().set_surface_override_material(0, &material);
            }
            Err(e) => godot_error!("Failed to cast resource to Material: {:?}", e),
        }
    }

    fn process(&mut self, delta: f64) {
        // Input handling
        let event = Input::singleton();

        // Get the dragging state
        let mut dragging = DRAGGING.write().unwrap();

        // Check if left click is pressed
        if event.is_action_pressed("left_click") {
            *dragging = true;
        } else if event.is_action_just_released("left_click") {
            *dragging = false;
        }

        // Rotate the mesh if dragging
        if *dragging {
            let pos = self.base().get_viewport().unwrap().get_mouse_position();

            let diff = pos - *MOUSE_POS.read().unwrap();

            self.base_mut().rotate_y((diff.x / 2.0) * delta as f32);
            self.base_mut().rotate_z((diff.y / 2.0) * delta as f32);
        }
        // Store previous position for the next frame
        *MOUSE_POS.write().unwrap() = self.base().get_viewport().unwrap().get_mouse_position();
    }
}

#[godot_api]
impl TerrainMesh {
    #[func]
    fn start_physics(&self) {
        let mut thread = THREAD.lock().unwrap();
        if thread.is_some() {
            return;
        }

        // Get all data for the thread
        let capacity = self.capacity as f16;
        let gravity = self.gravity as f16;
        let inertia = self.inertia;
        let diameter = self.diameter;
        let erosion_factor = self.erosion_factor as f16;
        let deposition_factor = self.deposition_factor as f16;
        let lifetime = self.lifetime;
        let starting_mass = self.starting_mass as f16;

        let (sender, reciever) = channel::<()>();

        *thread = Some((
            std::thread::spawn(move || {
                godot_print!("Starting physics thread");
                // Get the RenderingServer singleton
                let mut vs: Gd<RenderingServer> = RenderingServer::singleton();

                // Get the texture's dimensions
                let dims = *DIMS.read().unwrap();

                // Start a counter for the number of iterations
                let mut counter: usize = 0;

                // Get the texture data and clone it into a new Arc for async purposes
                let texture = TEXTURE.read().unwrap().clone();
                let texture_rwlock = RwLock::new(texture);
                let texture_arc = Arc::new(texture_rwlock);

                godot_print!("Starting physics loop");
                loop {
                    // Get the current time for iteration speed testing
                    let start = SystemTime::now();

                    // Wait 10ms to see if the kill signal is received
                    if reciever.recv_timeout(Duration::from_millis(10)).is_ok() {
                        godot_print!("Stopping physics thread");
                        break;
                    }

                    // Create Raindrops
                    let mut drops: Vec<Raindrop> = create_raindrops(20_000, starting_mass, *DIMS.read().unwrap());

                    // Simulate Raindrops
                    // Using the map function - add/remove the `par_` to add/remove parallelism
                    let changes: Vec<(f16, usize)> = drops
                        .par_iter_mut()
                        .map(|drop| drop.simulate(
                            Arc::clone(&texture_arc), *DIMS.read().unwrap(),
                            gravity,
                            capacity,
                            inertia,
                            erosion_factor,
                            deposition_factor,
                            diameter,
                            lifetime,
                        ))
                        .flatten()
                        .collect();

                    // Get mutable access to the texture
                    let mut texture = texture_arc.write().unwrap();

                    // Update the texture with the changes
                    for change in changes.iter() {
                        texture[change.1] += change.0;
                    }

                    // Get the end time for iteration speed testing
                    let duration = SystemTime::now().duration_since(start).unwrap();
                    godot_print!(
                        "Iteration {counter} took: {duration:?}, to calculate the changes."
                    );

                    // Update the texture in Godot
                    update_texture(
                        &texture,
                        (dims.0 as i32, dims.1 as i32),
                        *IMAGE_ID.read().unwrap(),
                        &mut vs,
                    );
                    counter += 1;

                    // Get the end time for iteration speed testing
                    let duration = SystemTime::now().duration_since(start).unwrap();
                    godot_print!(
                        "Iteration {counter} took: {duration:?}, made {} changes.",
                        changes.len()
                    );
                }

                // Update the texture in global state
                let texture_lock = texture_arc.read().unwrap().clone();

                // Import here, otherwise we get weird errors :|
                // I think this is due to exr having traits that effect Vectors.
                use exr::prelude::write_rgb_file;

                // Output the image
                write_rgb_file("output.exr", dims.0, dims.1, |x, y| {
                    let index = y * dims.0 + x;
                    let r = texture_lock[index] as f32;

                    (r, r, r)
                })
                .unwrap();

                // Save the texture to the global state
                let mut global_texture_lock = TEXTURE.write().unwrap();
                *global_texture_lock = texture_lock;
            }),
            sender,
        ));
    }

    #[func]
    /// Takes the thread out of the `Mutex` and drops it - forcing the thread to stop.
    fn stop_physics() {
        if let Some((thread, sender)) = THREAD.lock().unwrap().take() {
            sender.send(()).unwrap();
            thread.join().unwrap();
        }
    }
}

/// Updates the texture with the new height data
fn update_texture(texture: &[f16], dims: (i32, i32), image_id: Rid, rs: &mut Gd<RenderingServer>) {
    // Create a new PackedByteArray from the texture data
    let mut array = PackedByteArray::new();
    array.extend(
        texture
            .iter()
            .flat_map(|&x| x.to_le_bytes())
            .collect::<Vec<u8>>(),
    );

    // Create a new Image from the texture data
    let image = Image::create_from_data(dims.0, dims.1, false, Format::RH, &array).unwrap();

    // Update the texture using a RenderingServer singleton
    rs.texture_2d_update(image_id, &image, 0);
}
