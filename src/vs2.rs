    #[derive(VulkanoShader)]
    #[ty = "vertex"]
    #[src = "

#version 450

#extension GL_ARB_separate_shader_objects : enable
#extension GL_ARB_shading_language_450pack : enable

layout(location = 0) in vec2 position;

layout(push_constant) uniform pushConstants {
    vec2 aspect;
    vec2 offset;
    float zoom;
} push_const;

//layout(location = 0) out vec3 pos;

void main() {
    vec2 off = 2*(push_const.offset - vec2(0.5, 0.5));
    vec2 pos = (position - off)*push_const.aspect*push_const.zoom;
    gl_Position = vec4(pos, 0.0, 1.0);
}

"]
    #[allow(dead_code)]
    struct Dummy;