// @Title: Monocromo
// @Icon: circle-half
// @Color: #94a3b8
// @Tag: BW
// @Desc: Escala de grises para máxima concentración.

#version 300 es
precision highp float;

in vec2 v_texcoord;
out vec4 fragColor;
uniform sampler2D tex;

void main() {
    vec4 c = texture(tex, v_texcoord);

    // Fórmula estándar de luminancia (NTSC/PAL)
    // El ojo humano percibe el verde más brillante que el azul,
    // por eso usamos estos pesos:
    float gray = dot(c.rgb, vec3(0.299, 0.587, 0.114));

    // Creamos un nuevo color usando el valor gris para R, G y B
    fragColor = vec4(vec3(gray), c.a);
}
