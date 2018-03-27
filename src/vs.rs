#![cfg_attr(feature = "cargo-clippy", allow(empty_line_after_outer_attr))]
#![cfg_attr(feature = "cargo-clippy", allow(expl_impl_clone_on_copy))]

#[derive(VulkanoShader)]
#[ty = "vertex"]
#[src = "
#version 450
layout(location = 0) in vec2 position;
layout(push_constant) uniform pushConstants {
    vec2 aspect;
    vec2 offset;
    float zoom;
} push_const;

layout(location = 0) out vec2 tex_coords;
void main() {
    gl_Position = vec4(position, 0.0, 1.0);
    vec2 temp = 0.5*position/push_const.aspect/push_const.zoom + push_const.offset;
    tex_coords = temp;
    //tex_coords = vec2(0, 1) + vec2(1, -1)*temp;
}
"]
#[allow(dead_code)]
struct Dummy;