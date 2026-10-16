//! Build entrypoint for the shared operating-system utility stand-in.
//! The interpreter and native process/environment setup live in the fixture OS layer.

#[path = "os/stand_in.rs"]
mod os;

fn main() {
    os::run();
}
