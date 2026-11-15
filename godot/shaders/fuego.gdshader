// Llamas del fogón: planos cruzados con ruido animado y mezcla aditiva.
// Sin billboard (en VR cada ojo vería un giro distinto).
shader_type spatial;
render_mode blend_add, unshaded, cull_disabled, depth_draw_never, shadows_disabled, fog_disabled;

uniform sampler2D ruido : filter_linear_mipmap, repeat_enable;
uniform vec4 color_base : source_color = vec4(1.0, 0.32, 0.04, 1.0);
uniform vec4 color_punta : source_color = vec4(1.0, 0.78, 0.30, 1.0);
uniform float intensidad = 2.2;
uniform float velocidad = 1.25;
uniform float semilla = 0.0;

// Variación por plano: cada lengua de fuego está rotada distinto.
varying float var_semilla;

void vertex() {
	var_semilla = semilla + MODEL_MATRIX[0].x * 3.1 + MODEL_MATRIX[0].z * 1.7;
}

void fragment() {
	float semilla_v = var_semilla;
	// En QuadMesh, UV.y = 0 arriba y 1 abajo.
	float altura = 1.0 - UV.y;
	float x = UV.x - 0.5;
	float n1 = texture(ruido, vec2(UV.x * 0.9 + semilla_v, UV.y * 0.8 + TIME * velocidad)).r;
	float n2 = texture(ruido, vec2(UV.x * 1.7 - semilla_v * 0.5, UV.y * 1.4 + TIME * velocidad * 1.7)).r;
	float n = n1 * 0.65 + n2 * 0.35;
	// silueta de lengua de fuego que se angosta hacia arriba
	float ancho = mix(0.42, 0.04, pow(altura, 0.8));
	// (smoothstep con bordes invertidos es indefinido en GLSL: uso 1 - smoothstep)
	float forma = 1.0 - smoothstep(ancho * 0.35, ancho, abs(x + (n - 0.5) * 0.25 * altura));
	float corte = smoothstep(0.0, 0.08, altura) * (1.0 - smoothstep(0.55 + n * 0.3, 1.0, altura));
	float m = clamp(forma * corte * (0.55 + n * 0.9) - altura * 0.25, 0.0, 1.0);
	vec3 col = mix(color_base.rgb, color_punta.rgb, clamp(m * 1.3 - altura * 0.3, 0.0, 1.0));
	ALBEDO = col * intensidad * m;
	ALPHA = m;
}
