# cam-vis

A simple camera visualization tool using `v4l2` and Vulkan.

## Usage

```sh
$ ./cam-vis --help
cam-vis 0.1.0
Artyom Pavlov<newpavlov@gmail.com>
Simple camera visualization tool

USAGE:
    cam-vis [OPTIONS] <camera>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -g, --grid-step <grid_step>    Grid step in pixels [default: 64]
    -m, --mode <mode>              Vulkan present mode: immediate, mailbox, fifo or relaxed [default: fifo]

ARGS:
    <camera>    Path to camera device
```

## Controls

You can zoom and drag image using mouse. Additionally the following hotkeys
are available:

- `s`: save current frame as a PNG image.
- `g`: turn grid on or off.
- `h`: turn histogram on or off
- `r`: fit image into the current window size (reset drag and zoom).
- `Space`: pause on current frame.
- `Esc`: exit the application.

## Installation

- Install [Rust Programming Language](https://www.rust-lang.org/).
- Run `cargo build --release`.
- Compiled binary will be saved to `target/release/cam-vis`.
- You may need to install Vulkan related packages via your OS package manager.

## License

The application is licensed under either of

 * [Apache License, Version 2.0](http://www.apache.org/licenses/LICENSE-2.0)
 * [MIT license](http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
