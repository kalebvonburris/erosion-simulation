use std::sync::{Arc, RwLock};

use nalgebra::Vector2;

#[derive(Debug)]
pub struct Raindrop {
    // The mass contained by the drop
    pub sediment: f32,
    // How much water the drop contains
    pub water: f32,
    // The position of the drop
    pub position: Vector2<f32>,
    // The velocity of the drop
    velocity: f32,
    // The direction of the drop
    direction: Vector2<f32>,
    // The state alive/dead state of the droplet
    alive: bool,
}

impl Raindrop {
    /// Create a new Raindrop with the given mass
    pub fn new(starting_mass: f32, x: f32, y: f32) -> Self {
        Raindrop {
            sediment: 0.0,
            water: starting_mass,
            position: Vector2::new(x, y),
            velocity: 1.0,
            direction: Vector2::new(0.0, 0.0),
            alive: true,
        }
    }

    /// Simulate the `Raindrop` until it dies by setting `self.alive` to false.
    ///
    /// # Arguments
    ///
    /// * `texture` - The texture to simulate on as a `&[f32]`.
    /// * `dims` - The dimensions of the texture as a tuple of `(x: usize, y: usize)`.
    ///
    /// # Returns
    ///
    /// A vector of tuples containing the amount of material deposited/eroded (based on sign) and the x/y coordinates.
    /// Contained a the tuple `(material: f32, x: usize, y: usize)`.
    pub fn simulate(
        &mut self,
        texture: Arc<RwLock<Vec<f32>>>,
        dims: (usize, usize),
        gravity: f32,
        capacity: f32,
        inertia: f32,
        erosion_factor: f32,
        deposition_factor: f32,
        diameter: f32,
        lifetime: u32,
    ) -> Vec<(f32, usize)> {
        // Create a vector to store changes
        let mut changes = Vec::with_capacity(
            (diameter.powi(2) * std::f32::consts::PI / 4.0).ceil() as usize * lifetime as usize,
        );

        // Grab a read lock on the texture
        let texture = texture.read().unwrap();

        for _ in 0..lifetime {
            // Store current position for later
            let prev_x = self.position.x;
            let prev_y = self.position.y;

            // Find slope of the terrain at the Raindrop's position
            let (starting_height, gradient) =
                get_height_and_gradient(self.position, &texture, dims);

            // Find the new direction of the Raindrop - normalize so we only step exactly 1 unit
            self.direction =
                ((self.direction * inertia) - (gradient * (1.0 - inertia))).normalize();

            // Set the droplet to the new position
            self.position += self.direction;

            // If the Raindrop is out of bounds, reflect it
            if self.position.x >= (dims.0 - 1) as f32 {
                self.kill(dims, diameter);
                break;
            } else if self.position.x < 0.0 {
                self.kill(dims, diameter);
                break;
            }
            // Reflection for y case
            if self.position.y >= (dims.1 - 1) as f32 {
                self.kill(dims, diameter);
                break;
            } else if self.position.y < 0.0 {
                self.kill(dims, diameter);
                break;
            }

            // Get the height of the new position
            let (height, _) = get_height_and_gradient(self.position, &texture, dims);

            // Get the height difference
            let diff = height - starting_height;
            // Calculate the 'c' sediment capacity
            let sediment_capacity = (-diff).min(0.05) * self.velocity * self.water * capacity;

            if self.sediment > sediment_capacity || diff > 0.0 {
                // If we carry more sediment than the capacity or are moving downhill, deposit it
                let deposit = if diff > 0.0 {
                    diff.min(self.sediment)
                } else {
                    (self.sediment - sediment_capacity) * deposition_factor
                };

                changes.append(&mut self.erode_deposit(
                    dims,
                    Vector2::new(prev_x, prev_y),
                    diameter,
                    deposit,
                ));
            } else {
                // Erode the sediment
                // Use a negative value to indicate erosion
                let deposit = -((sediment_capacity - self.sediment) * erosion_factor).min(-diff);
                changes.append(&mut self.erode_deposit(
                    dims,
                    Vector2::new(prev_x, prev_y),
                    diameter,
                    deposit,
                ));
            }

            // Calculate the new velocity
            self.velocity = (self.velocity.powi(2) + diff * gravity).sqrt().max(0.0001);
            self.water *= 0.99;

            if self.velocity <= 0.01 {
                self.kill(dims, diameter);
                break;
            }
        }

        // godot_print!(
        //     "Returning {} changes, {} alloc'd",
        //     changes.len(),
        //     changes.capacity()
        // );

        changes
    }

    /// Modifies the given texture by a quadratic function for depositing/removing material.
    ///
    /// # Arguments
    ///
    /// * `dims` - The dimensions of the texture as a tuple of `(usize, usize)`.
    /// * `diameter` - The diameter the `Raindrop` covers.
    /// * `deposit` - The amount of material to deposit - can be negative to erode.
    ///
    /// # Returns
    ///
    /// A vector of tuples containing the amount of material deposited/eroded (based on sign) and the x/y coordinates.
    /// Contained a the tuple `(material: f32, x: usize, y: usize)`.
    ///
    /// # Explanation
    ///
    /// We know what square we're in with `self.position` - so we create a square// Deposit the material - check bounds
    /// with diameter `diameter` around the `Raindrop`'s position, and then iterate
    /// over them using a quadratic function to determine how much material to deposit.
    ///
    /// Because we have a diameter, we take the distance from `self.position` to all
    /// points within range to establish a weight. Negative weights are ignored, and
    /// positive weights are calculated using `(1 - weight)^2 * deposit`. These weights
    /// are summed together and the sum is used to divide out the deposit among the
    /// points.
    pub fn erode_deposit(
        &mut self,
        dims: (usize, usize),
        position: Vector2<f32>,
        diameter: f32,
        deposit: f32,
    ) -> Vec<(f32, usize)> {
        // Get the height of the square - rounding up so we don't miss points
        let height = diameter.ceil() as usize;

        // Since we're using a square, we'll need at most `(diameter + 1)^2` points.
        //  Each point is a tuple of `(weight, x, y)`.
        // There's some weird alloc capacity - this is because a sphere fills a circle
        //  at a ratio of 4:pi, so we can use an approximation
        let mut points: Vec<(f32, usize)> =
            Vec::with_capacity((diameter.powi(2) * std::f32::consts::PI / 4.0).ceil() as usize);

        // Sum the weights of the points
        let mut weight_sum = 0.0;

        // Iterate over the points in the square - checking they're in bounds
        for y in 0..=height {
            // Skip columns that are out of bounds
            let y_offset = diameter / 2.0 - y as f32;
            if position.y - y_offset < 0.0 || position.y - y_offset >= dims.1 as f32 {
                continue;
            }
            for x in 0..=height {
                // Skip rows that are out of bounds
                let x_offset = diameter / 2.0 - x as f32;
                if position.x - x_offset < 0.0 || position.x - x_offset >= dims.0 as f32 {
                    continue;
                }

                // Get the point - we have to floor it for integer indexing
                let x = (position.x - x_offset).floor();
                let y = (position.y - y_offset).floor();

                // Get the distance from the center
                let distance = ((x - position.x).powi(2) + (y - position.y).powi(2)).sqrt();

                // Skip points that are out of the circle
                if distance > diameter / 2.0 {
                    continue;
                }

                // We now have a point within the circle - calculate the weight
                let weight = (1.0 - (distance / (diameter / 2.0))).powi(2);

                // Push the point and weight to the vector
                points.push((weight, y as usize * dims.0 + x as usize));

                // Add the weight to the sum
                weight_sum += weight;
            }
        }

        // Iterate over the points and deposit material
        // We can use an iter mut to reuse the points vector, saving
        // an entire set of allocations
        for (weight, _) in points.iter_mut() {
            // Calculate the deposit
            let deposit = deposit * *weight / weight_sum;

            // Modify the deposit based on the height
            *weight = deposit;

            // Remove sediment from the Raindrop
            self.sediment -= deposit;
        }

        points
    }

    /// Kills the `Raindrop`.
    ///
    /// This is a separate function because there may need to be additional logic.
    pub fn kill(&mut self, dims: (usize, usize), diameter: f32) {
        self.alive = false;
        self.erode_deposit(dims, self.position, diameter, self.sediment);
    }
}

/// Get the height and gradient of a point in the texture.
///
/// Returns a tuple containing the height and the 2D gradient vector.
fn get_height_and_gradient(
    point: Vector2<f32>,
    texture: &[f32],
    dims: (usize, usize),
) -> (f32, Vector2<f32>) {
    // Get the index of the grid point - this just makes it cleaner :)
    let texture_index = point.y as usize * dims.0 + point.x as usize;

    // Get the u/v offset values from the top right of the grid point
    let u = point.x.fract();
    let v = point.y.fract();

    // Get the heights of the four corners of the grid point
    let nw = texture[texture_index];
    let ne = texture[texture_index + 1];
    let sw = texture[texture_index + dims.0];
    let se = texture[texture_index + dims.0 + 1];

    // Calculate the gradient
    let gradient: Vector2<f32> = Vector2::new(
        ((ne - nw) * (1.0 - v)) + ((se - sw) * v),
        ((sw - nw) * (1.0 - u)) + ((se - ne) * u),
    );

    let height =
        (nw * (1.0 - u) * (1.0 - v)) + (ne * u * (1.0 - v)) + (sw * (1.0 - u) * v) + (se * u * v);

    (height, gradient)
}
