// @Title: OLED Puro
// @Icon: square
// @Color: #000000
// @Tag: DARK
// @Desc: Negros absolutos para pantallas OLED.

#version 300 es
precision highp float;

in vec2 v_texcoord;
out vec4 fragColor;
uniform sampler2D tex;

void main() {
    // 1. Muestreo moderno
    vec4 col = texture(tex, v_texcoord);

    // 2. Lógica de umbral (Threshold)
    // 'length(col.rgb)' mide la intensidad total del color.
    // Si la intensidad es menor a 0.1, lo forzamos a negro absoluto.
    if(length(col.rgb) < 0.1) {
        col.rgb = vec3(0.0);
    }

    // 3. Salida
    fragColor = col;
}
