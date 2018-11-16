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
    vec2 t1 = position/2/push_const.aspect/push_const.zoom;
    tex_coords = t1 - push_const.offset + 0.5;
}