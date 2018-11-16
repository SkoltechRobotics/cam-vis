use vulkano::swapchain::PresentMode;

#[derive(StructOpt)]
#[structopt(
    name = "cam-vis",
    about = "Simple camera visualization tool")]
pub(crate) struct Cli {
    /// Path to camera device
    pub camera: String,
    #[structopt(long = "mode", short = "m",
        parse(try_from_str = "parse_mode"),
        default_value="mailbox")]
    /// Vulkan present mode: immediate, mailbox, fifo or relaxed
    pub mode: PresentMode,
    #[structopt(long = "grid-step", short = "g", default_value="64")]
    /// Grid step in pixels
    pub grid_step: u32,
}

fn parse_mode(s: &str) -> Result<PresentMode, &'static str> {
    use self::PresentMode::*;

    Ok(match s {
        "immediate" => Immediate,
        "mailbox" => Mailbox,
        "fifo" => Fifo,
        "relaxed" => Relaxed,
        _ => Err("unknown present mode")?,
    })
}
