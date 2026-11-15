// Pasto y cortaderas instanciados con MultiMesh. El viento desplaza las
// puntas (UV.y = altura relativa). La normal se fuerza hacia arriba para que
// el pasto se ilumine como el suelo y las caras traseras no se oscurezcan.
shader_type spatial;
render_mode cull_disabled, diffuse_lambert, specular_disabled, world_vertex_coords;

uniform float fuerza_viento = 0.10;
uniform float velocidad_viento = 1.4;
uniform vec2 direccion_viento = vec2(0.8, 0.6);
// Intensidad de la ráfaga (0..1), la actualiza Rust en cada cuadro.
uniform float rafaga = 0.5;
uniform vec3 tinte : source_color = vec3(1.0);
uniform float oscuridad_base = 0.5;

vec3 a_lineal(vec3 c) {
	return c * (c * (c * 0.305306011 + 0.682171111) + 0.012522878);
}

void vertex() {
	vec3 origen = MODEL_MATRIX[3].xyz;
	float h = UV.y;
	float fase = dot(origen.xz, vec2(0.21, 0.17)) + TIME * velocidad_viento;
	float ola = sin(fase) * 0.6 + sin(fase * 2.3 + origen.x * 0.5) * 0.25;
	float flex = h * h;
	vec2 dir = normalize(direccion_viento);
	float empuje = (ola * 0.5 + 0.5) * (0.4 + rafaga) + rafaga * 0.5;
	VERTEX.xz += dir * empuje * fuerza_viento * flex;
	VERTEX.y -= empuje * fuerza_viento * flex * 0.25;
}

void fragment() {
	vec3 c = a_lineal(COLOR.rgb) * tinte;
	// Cel: la sombra de la mata en tres bandas planas en vez de un degradé.
	float banda = floor(UV.y * 3.0 + 0.35) / 3.0;
	ALBEDO = c * mix(oscuridad_base, 1.0, clamp(banda, 0.0, 1.0));
	NORMAL = normalize((VIEW_MATRIX * vec4(0.0, 1.0, 0.0, 0.0)).xyz);
	ROUGHNESS = 1.0;
}
