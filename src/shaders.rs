pub mod fs {
    vulkano_shaders::shader!{
        ty: "fragment",
        path: "src/shaders/fs.glsl"
    }
}

pub mod fs2 {
    vulkano_shaders::shader!{
        ty: "fragment",
        path: "src/shaders/fs2.glsl"
    }
}

pub mod vs {
    vulkano_shaders::shader!{
        ty: "vertex",
        path: "src/shaders/vs.glsl"
    }
}

pub mod vs2 {
    vulkano_shaders::shader!{
        ty: "vertex",
        path: "src/shaders/vs2.glsl"
    }
}
