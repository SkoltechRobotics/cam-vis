#![cfg_attr(feature = "cargo-clippy", allow(empty_line_after_outer_attr))]
#![cfg_attr(feature = "cargo-clippy", allow(expl_impl_clone_on_copy))]

#[derive(VulkanoShader)]
#[ty = "vertex"]
#[src = "
#version 450

// #extension GL_ARB_separate_shader_objects : enable
// #extension GL_ARB_shading_language_450pack : enable

layout(location = 0) in vec2 position;

layout(push_constant) uniform pushConstants {
    vec2 aspect;
    vec2 offset;
    float zoom;
} push_const;

// layout(location = 0) out vec3 pos;

void main() {
    vec2 pos = (position + 2*push_const.offset)*push_const.aspect*push_const.zoom;
    gl_Position = vec4(pos, 0.0, 1.0);
}
"]
#[allow(dead_code)]
struct Dummy;