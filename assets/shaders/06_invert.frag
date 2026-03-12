// @Title: Inverted
// @Icon: repeat
// @Color: #ffffff
// @Tag: INV
// @Desc: Inverts colors (High Contrast Mode).

#version 300 es
precision highp float;

in vec2 v_texcoord;       // Formerly: varying
out vec4 fragColor;       // Formerly: gl_FragColor (Now manually defined)
uniform sampler2D tex;

void main() {
    // Formerly: texture2D -> Now: texture
    vec4 pixColor = texture(tex, v_texcoord);

    // Mathematical logic remains the same, only the "plumbing" is updated
    vec3 inverted = 1.0 - pixColor.rgb;

    fragColor = vec4(inverted, pixColor.a);
}