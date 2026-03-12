// @Title: Pure OLED
// @Icon: square
// @Color: #000000
// @Tag: DARK
// @Desc: Absolute blacks optimized for OLED screens.

#version 300 es
precision highp float;

in vec2 v_texcoord;
out vec4 fragColor;
uniform sampler2D tex;

void main() {
    // 1. Modern sampling
    vec4 col = texture(tex, v_texcoord);

    // 2. Threshold Logic
    // 'length(col.rgb)' measures total color intensity.
    // If the intensity is below 0.1, we force it to absolute black.
    if(length(col.rgb) < 0.1) {
        col.rgb = vec3(0.0);
    }

    // 3. Output
    fragColor = col;
}