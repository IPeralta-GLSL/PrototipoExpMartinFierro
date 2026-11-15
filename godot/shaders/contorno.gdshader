// Contorno de los objetos interactivos (casco invertido, ancho constante en
// píxeles): se agrega como material_overlay a las mallas del objeto.
shader_type spatial;
render_mode unshaded, cull_front, shadows_disabled, depth_draw_never, blend_mix, fog_disabled;

uniform vec4 color : source_color = vec4(1.0, 0.9, 0.55, 1.0);
uniform float alfa = 1.0;
uniform float grosor = 3.0; // píxeles

void vertex() {
	vec4 clip = PROJECTION_MATRIX * (MODELVIEW_MATRIX * vec4(VERTEX, 1.0));
	vec3 n = normalize((MODELVIEW_MATRIX * vec4(NORMAL, 0.0)).xyz);
	vec2 dir = (PROJECTION_MATRIX * vec4(n, 0.0)).xy;
	float largo = length(dir);
	dir = largo > 1e-6 ? dir / largo : vec2(0.0);
	clip.xy += dir * grosor * 2.0 / VIEWPORT_SIZE * clip.w;
	POSITION = clip;
}

void fragment() {
	ALBEDO = color.rgb;
	ALPHA = color.a * alfa;
}
