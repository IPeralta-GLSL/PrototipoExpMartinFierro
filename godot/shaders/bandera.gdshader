// Bandera argentina que flamea: franjas celeste-blanca-celeste con el sol.
// Un plano subdividido (UV.x = 0 junto al mástil) que ondula con el viento.
shader_type spatial;
render_mode cull_disabled;

uniform vec3 celeste : source_color = vec3(0.45, 0.68, 0.87);
uniform vec3 blanco : source_color = vec3(0.95, 0.94, 0.9);
uniform vec3 oro : source_color = vec3(0.93, 0.7, 0.18);
uniform float viento = 1.0;

void vertex() {
	float t = TIME * 2.4;
	float u = UV.x;
	float onda = sin(u * 7.0 - t) * 0.09 + sin(u * 13.0 - t * 1.7 + UV.y * 3.0) * 0.03;
	VERTEX.z += onda * u * viento;
	VERTEX.y -= u * u * 0.06;
	NORMAL = normalize(vec3(-cos(u * 7.0 - t) * 0.6 * u, 0.0, 1.0));
}

void fragment() {
	vec3 c = (UV.y > 1.0 / 3.0 && UV.y < 2.0 / 3.0) ? blanco : celeste;
	vec2 sol = (UV - vec2(0.5)) * vec2(1.6, 1.0);
	if (length(sol) < 0.085) {
		c = oro;
	}
	ALBEDO = c;
	ROUGHNESS = 0.9;
	// Un poco de luz propia para que se vea de noche junto al fuego.
	EMISSION = c * 0.12;
}
