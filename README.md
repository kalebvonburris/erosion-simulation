# Hydraulic Erosion

Hydraulic Erosion is the process by which terrain is altered by the remove and deposition of soil caused by the flow of water. In this case, I'm using a particle simulation using a `Raindrop` type. Performance here is significantly better than I initially expected, with ~20,000 drops on a 512x512 map taking ~200ms on my machine.

## Getting it Running

In order to build the application, you'll need Rust (see [rustup.rs](rustup.rs)).

From there, build the dynamic library for the Godot project by running the following command in the `rust` folder:

```console
cargo build --release
```

You should now be able to open the Godot project and run the simulation by pressing the play button or `F5`.

## Sources

I took great inspiration from [this](https://www.youtube.com/watch?v=eaXk97ujbPQ) video by Sebatian Lague where he does much the same process in Unity using C# and eventually compute shaders.

I also attached that he used as reading it helped me understand the small problems I was having that weren't happening in Sebatian's code, which can be found [here](https://github.com/SebLague/Hydraulic-Erosion).
