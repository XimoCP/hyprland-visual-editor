// @Title: Sharpen
// @Icon: aperture
// @Color: #22d3ee
// @Tag: SHARP
// @Desc: Enhances edges and text clarity (Sharpen).

#version 300 es
precision highp float;

in vec2 v_texcoord;
out vec4 fragColor;
uniform sampler2D tex;

void main() {
    // 1. Sample the central pixel (Modern sampling)
    vec4 center = texture(tex, v_texcoord);

    // 2. Define the offset
    // Note: 0.0005 is ideal for 1080p.
    // For 4K displays, you might need to increase it to 0.001.
    float offset = 0.0005;

    // 3. Sample adjacent pixels (texture instead of texture2D)
    vec3 up    = texture(tex, v_texcoord + vec2(0.0, offset)).rgb;
    vec3 down  = texture(tex, v_texcoord - vec2(0.0, offset)).rgb;
    vec3 left  = texture(tex, v_texcoord - vec2(offset, 0.0)).rgb;
    vec3 right = texture(tex, v_texcoord + vec2(offset, 0.0)).rgb;

    // 4. Sharpening Kernel (Laplacian Sharpening)
    // We multiply the center by 5 and subtract the neighbor cross.
    // This amplifies contrast differences at the edges.
    vec3 result = center.rgb * 5.0 - (up + down + left + right);

    // 5. Output with safety clamp
    // Clamping is vital here as the subtraction could yield negative values.
    fragColor = vec4(clamp(result, 0.0, 1.0), center.a);
}