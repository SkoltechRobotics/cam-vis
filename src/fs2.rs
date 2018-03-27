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