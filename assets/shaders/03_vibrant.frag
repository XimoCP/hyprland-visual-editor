// @Title: Vibrante
// @Icon: sun
// @Color: #facc15
// @Tag: SAT
// @Desc: Aumenta la saturación y el contraste.

#version 300 es
precision highp float;

in vec2 v_texcoord;
out vec4 fragColor;
uniform sampler2D tex;

void main() {
    // 1. Captura de color moderna
    vec4 col = texture(tex, v_texcoord);

    // 2. Cálculo de saturación actual
    // Buscamos la diferencia entre el canal más fuerte y el más débil
    float max_val = max(col.r, max(col.g, col.b));
    float min_val = min(col.r, min(col.g, col.b));
    float sat = max_val - min_val;

    // 3. Luminancia estándar (Rec. 709)
    // Usamos los coeficientes modernos para mayor precisión en pantallas digitales:
    // $$L = 0.2126R + 0.7152G + 0.0722B$$
    float luma = dot(col.rgb, vec3(0.2126, 0.7152, 0.0722));

    // 4. Lógica de Vibrancia
    // Intensidad: 0.8. Cuanto más bajo sea 'sat', más 'amount' aplicaremos.
    float vibrance = 0.8;
    float amount = vibrance * (1.0 - sat);

    // Mezclamos la luminancia con el color original basándonos en el cálculo inteligente
    vec3 result = mix(vec3(luma), col.rgb, 1.0 + amount);

    // 5. Salida
    fragColor = vec4(result, col.a);
}
