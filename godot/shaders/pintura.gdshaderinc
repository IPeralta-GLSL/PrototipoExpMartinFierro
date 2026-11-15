// Cel shading pictórico (lo incluyen pintura.gdshader y pintura_doble.gdshader).
// - Luz en dos bandas; el borde entre luz y sombra sigue las pinceladas.
// - Pinceladas en el color, fijas al objeto (en su espacio, proyección triplanar):
//   no se deslizan al moverse ni al caminar.
// Rust (pintura.rs) convierte a esto los StandardMaterial3D opacos de la escena.

uniform vec4 color : source_color = vec4(1.0);
uniform sampler2D textura : source_color, hint_default_white, filter_linear_mipmap, repeat_enable;
uniform bool usar_color_vertice = false;
uniform vec3 uv1_escala = vec3(1.0);
uniform vec3 uv1_desplazamiento = vec3(0.0);
uniform sampler2D pincel : hint_default_white, filter_linear_mipmap, repeat_enable;
uniform float escala_pincel = 4.0;   // trazos por metro
uniform float fuerza_pincel = 0.16;  // cuánto cambian el color
uniform float borde_pincel = 0.45;   // cuánto ondula el borde de la sombra
uniform float suavidad = 0.05;       // ancho del corte entre luz y sombra

varying vec3 pos_obj;
varying vec3 normal_obj;
varying float trazo;

void vertex() {
	pos_obj = VERTEX;
	normal_obj = NORMAL;
	UV = UV * uv1_escala.xy + uv1_desplazamiento.xy;
}

// Trazos alargados proyectados desde los tres ejes según la normal.
float pinceladas(vec3 p, vec3 n) {
	vec3 w = pow(abs(n), vec3(4.0));
	w /= max(w.x + w.y + w.z, 1e-4);
	vec2 e = vec2(1.0, 3.2) * escala_pincel;
	float a = texture(pincel, p.zy * e).r;
	float b = texture(pincel, p.xz * e.yx).r;
	float c = texture(pincel, p.xy * e).r;
	return a * w.x + b * w.y + c * w.z;
}

void fragment() {
	vec4 base = texture(textura, UV) * color;
	if (usar_color_vertice) {
		base *= COLOR;
	}
	trazo = pinceladas(pos_obj, normalize(normal_obj));
	ALBEDO = base.rgb * (1.0 + (trazo - 0.5) * 2.0 * fuerza_pincel);
	ROUGHNESS = 1.0;
	METALLIC = 0.0;
	SPECULAR = 0.0;
}

void light() {
	float ndl = dot(NORMAL, LIGHT);
	float umbral = (trazo - 0.5) * borde_pincel;
	float luz = smoothstep(umbral - suavidad, umbral + suavidad, ndl);
	DIFFUSE_LIGHT += luz * ATTENUATION * LIGHT_COLOR / PI;
}
