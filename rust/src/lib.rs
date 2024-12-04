#![feature(f16)]

use godot::prelude::*;
use raindrop::Raindrop;

pub mod raindrop;
pub mod terrain_mesh;

struct ErosionExtension;

#[gdextension]
unsafe impl ExtensionLibrary for ErosionExtension {}

/// Creates Raindrops at random points on the terrain
fn create_raindrops(num: usize, mass: f16, dims: (usize, usize)) -> Vec<Raindrop> {
    let mut drops: Vec<Raindrop> = Vec::with_capacity(num);

    while drops.len() < num {
        let x = rand::random::<usize>() % (dims.0 - 1);
        let y = rand::random::<usize>() % (dims.1 - 1);

        drops.push(Raindrop::new(mass, x as f32, y as f32));
    }

    drops
}
