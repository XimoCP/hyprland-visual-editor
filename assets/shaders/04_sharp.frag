// @Title: Nitidez
// @Icon: aperture
// @Color: #22d3ee
// @Tag: SHARP
// @Desc: Realza los bordes y textos (Sharpen).

#version 300 es
precision highp float;

in vec2 v_texcoord;
out vec4 fragColor;
uniform sampler2D tex;

void main() {
    // 1. Tomamos el píxel central (Muestreo moderno)
    vec4 center = texture(tex, v_texcoord);

    // 2. Definimos el desplazamiento
    // Nota: 0.0005 es ideal para 1080p.
    // Si usas 4K, podrías necesitar subirlo a 0.001.
    float offset = 0.0005;

    // 3. Muestreamos adyacentes (texture en lugar de texture2D)
    vec3 up    = texture(tex, v_texcoord + vec2(0.0, offset)).rgb;
    vec3 down  = texture(tex, v_texcoord - vec2(0.0, offset)).rgb;
    vec3 left  = texture(tex, v_texcoord - vec2(offset, 0.0)).rgb;
    vec3 right = texture(tex, v_texcoord + vec2(offset, 0.0)).rgb;

    // 4. Kernel de enfoque (Laplacian Sharpening)
    // Multiplicamos el centro por 5 y restamos la cruz de vecinos.
    // Esto amplifica las diferencias de contraste en los bordes.
    vec3 result = center.rgb * 5.0 - (up + down + left + right);

    // 5. Salida con recorte de seguridad (clamp)
    // El clamp es vital aquí porque la resta puede dar valores negativos
    fragColor = vec4(clamp(result, 0.0, 1.0), center.a);
}
