// Suelo de la pampa: color de vértice (generado en Rust) + detalle procedural.
shader_type spatial;
render_mode cull_back, diffuse_lambert, specular_disabled;

uniform sampler2D detalle : hint_default_white, filter_linear_mipmap, repeat_enable;
uniform float escala_detalle = 0.3;
uniform float fuerza_detalle = 0.45;
uniform vec3 tinte : source_color = vec3(1.0);

varying vec3 pos_mundo;

// sRGB -> lineal (aproximación barata). Los colores de vértice se
// definieron "a ojo" en sRGB.
vec3 a_lineal(vec3 c) {
	return c * (c * (c * 0.305306011 + 0.682171111) + 0.012522878);
}

void vertex() {
	pos_mundo = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
}

void fragment() {
	float d1 = texture(detalle, pos_mundo.xz * escala_detalle).r;
	float d2 = texture(detalle, pos_mundo.xz * escala_detalle * 0.11 + vec2(0.37, 0.71)).r;
	float d = mix(1.0, (d1 * 0.55 + d2 * 0.45) * 1.7, fuerza_detalle);
	ALBEDO = a_lineal(COLOR.rgb) * tinte * d;
	ROUGHNESS = 1.0;
}
