// Línea de tinta del cel shading (casco invertido, opaco): va como next_pass
// de los materiales de personas y animales. Ancho en píxeles, más fino de lejos.
shader_type spatial;
render_mode unshaded, cull_front, shadows_disabled, depth_draw_opaque, fog_disabled;

uniform vec4 color : source_color = vec4(0.07, 0.045, 0.03, 1.0);
uniform float grosor = 2.2; // píxeles, a corta distancia

void vertex() {
	vec4 clip = PROJECTION_MATRIX * (MODELVIEW_MATRIX * vec4(VERTEX, 1.0));
	vec3 n = normalize((MODELVIEW_MATRIX * vec4(NORMAL, 0.0)).xyz);
	vec2 dir = (PROJECTION_MATRIX * vec4(n, 0.0)).xy;
	float largo = length(dir);
	dir = largo > 1e-6 ? dir / largo : vec2(0.0);
	// A más de ~4 m la línea se afina (hasta el 35 %) para no empastar lo lejano.
	float ancho = grosor * clamp(4.0 / max(clip.w, 0.01), 0.35, 1.0);
	clip.xy += dir * ancho * 2.0 / VIEWPORT_SIZE * clip.w;
	POSITION = clip;
}

void fragment() {
	ALBEDO = color.rgb;
}
