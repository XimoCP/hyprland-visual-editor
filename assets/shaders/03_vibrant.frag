// @Title: Vibrant
// @Icon: sun
// @Color: #facc15
// @Tag: SAT
// @Desc: Boosts saturation and contrast intelligently.

#version 300 es
precision highp float;

in vec2 v_texcoord;
out vec4 fragColor;
uniform sampler2D tex;

void main() {
    // 1. Modern color capture
    vec4 col = texture(tex, v_texcoord);

    // 2. Current saturation calculation
    // Finding the difference between the strongest and weakest channels
    float max_val = max(col.r, max(col.g, col.b));
    float min_val = min(col.r, min(col.g, col.b));
    float sat = max_val - min_val;

    // 3. Standard Luminance (Rec. 709)
    // Using modern coefficients for digital displays:
    // $$L = 0.2126R + 0.7152G + 0.0722B$$
    float luma = dot(col.rgb, vec3(0.2126, 0.7152, 0.0722));

    // 4. Vibrance Logic
    // Intensity: 0.8. The lower the 'sat', the more 'amount' we apply.
    float vibrance = 0.8;
    float amount = vibrance * (1.0 - sat);

    // Mix the luminance with the original color based on the smart calculation
    vec3 result = mix(vec3(luma), col.rgb, 1.0 + amount);

    // 5. Output
    fragColor = vec4(result, col.a);
}