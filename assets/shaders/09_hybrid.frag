// @Title: Cyberpunk
// @Icon: cpu
// @Color: #d946ef
// @Tag: CYBER
// @Desc: Mezcla de contraste alto y tonos neón.

#version 300 es
precision highp float;

in vec2 v_texcoord;
out vec4 fragColor;
uniform sampler2D tex;

void main() {
    // 1. Captura de color moderna
    vec4 col = texture(tex, v_texcoord);

    // 2. Desaturación controlada (30%)
    // Calculamos el gris y lo mezclamos con el original para no perder todo el color
    float gray = dot(col.rgb, vec3(0.299, 0.587, 0.114));
    vec3 mixed = mix(col.rgb, vec3(gray), 0.3);

    // 3. Empuje de contraste (Mid-tone boost)
    // Usamos 0.5 como punto de pivote: lo claro se aclara, lo oscuro se oscurece
    mixed = mix(vec3(0.5), mixed, 1.2);

    // 4. Salida final
    fragColor = vec4(mixed, col.a);
}
