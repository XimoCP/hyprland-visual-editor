// @Title: Invertido
// @Icon: repeat
// @Color: #ffffff
// @Tag: INV
// @Desc: Invierte los colores (Modo Alto Contraste).

#version 300 es
precision highp float;

in vec2 v_texcoord;       // Antes: varying
out vec4 fragColor;       // Antes: gl_FragColor (¡Ahora lo definimos nosotros!)
uniform sampler2D tex;

void main() {
    // Antes: texture2D -> Ahora: texture
    vec4 pixColor = texture(tex, v_texcoord);

    // La lógica matemática NO cambia, solo la "fontanería"
    vec3 inverted = 1.0 - pixColor.rgb;

    fragColor = vec4(inverted, pixColor.a);
}
