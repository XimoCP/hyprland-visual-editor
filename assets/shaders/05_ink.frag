// @Title: Papel
// @Icon: file-text
// @Color: #fdba74
// @Tag: READ
// @Desc: Tono sepia suave para lectura prolongada.

#version 300 es
precision highp float;

in vec2 v_texcoord;
out vec4 fragColor;
uniform sampler2D tex;

// Función de ruido (Sigue funcionando igual en 300 es)
float rand(vec2 co){
    return fract(sin(dot(co.xy ,vec2(12.9898,78.233))) * 43758.5453);
}

void main() {
    // 1. Muestreo (texture en lugar de texture2D)
    vec4 color = texture(tex, v_texcoord);

    // 2. Escala de grises (Luminancia estándar)
    float gray = dot(color.rgb, vec3(0.299, 0.587, 0.114));

    // 3. Tinte sepia/papel
    // Multiplicamos el gris por un vector que tira al amarillo/crema
    vec3 sepia = vec3(gray) * vec3(1.0, 0.95, 0.82);

    // 4. Añadir ruido (grano de papel)
    // El ruido se suma al color final para dar esa sensación de textura
    float noise = (rand(v_texcoord) - 0.5) * 0.05;

    // 5. Salida final
    fragColor = vec4(sepia + noise, color.a);
}
