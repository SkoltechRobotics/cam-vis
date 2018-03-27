#![cfg_attr(feature = "cargo-clippy", allow(empty_line_after_outer_attr))]
#![cfg_attr(feature = "cargo-clippy", allow(expl_impl_clone_on_copy))]

#[derive(VulkanoShader)]
#[ty = "fragment"]
#[src = "
#version 450

//layout(location = 0) in vec2 position;
layout(location = 0) out vec4 f_color;

void main() {
f_color = vec4(0, 1, 0, 1);
}

"]
#[allow(dead_code)]
struct Dummy;
