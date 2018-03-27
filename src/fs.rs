#![cfg_attr(feature = "cargo-clippy", allow(empty_line_after_outer_attr))]
#![cfg_attr(feature = "cargo-clippy", allow(expl_impl_clone_on_copy))]

#[derive(VulkanoShader)]
#[ty = "fragment"]
#[src = "
#version 450
layout(location = 0) in vec2 tex_coords;
layout(location = 0) out vec4 f_color;
layout(set = 0, binding = 0) uniform sampler2D tex;
void main() {
    f_color = texture(tex, tex_coords);
}
"]
#[allow(dead_code)]
struct Dummy;