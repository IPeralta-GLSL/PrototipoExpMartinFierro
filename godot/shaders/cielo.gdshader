// Cielo pampeano: atardecer -> noche estrellada.
// `noche` (0..1) lo anima CicloDia (Rust). Sin TIME: el cielo solo se
// recalcula cuando cambian los parámetros (barato para el Quest).
// Las estrellas, la Vía Láctea y las Tres Marías se dibujan solo en el fondo
// (no en el cubemap de radiancia), así no ensucian la iluminación.
shader_type sky;

// --- Atardecer ---
uniform vec3 color_cenit : source_color = vec3(0.18, 0.29, 0.49);
uniform vec3 color_medio : source_color = vec3(0.62, 0.48, 0.58);
uniform vec3 color_horizonte : source_color = vec3(0.98, 0.64, 0.38);
uniform vec3 color_suelo_horizonte : source_color = vec3(0.78, 0.58, 0.37);
uniform vec3 color_suelo : source_color = vec3(0.29, 0.27, 0.2);
uniform float curva = 0.45;

// --- Noche ---
uniform float noche : hint_range(0.0, 1.0) = 0.0;
uniform float crepusculo : hint_range(0.0, 1.0) = 0.0;
uniform vec3 noche_cenit : source_color = vec3(0.012, 0.018, 0.05);
uniform vec3 noche_medio : source_color = vec3(0.025, 0.035, 0.08);
uniform vec3 noche_horizonte : source_color = vec3(0.06, 0.065, 0.11);
uniform vec3 noche_suelo : source_color = vec3(0.012, 0.012, 0.018);
uniform vec3 color_ocaso : source_color = vec3(0.95, 0.38, 0.16);
// Dirección (horizontal) por donde se puso el sol, para el resplandor.
uniform vec3 dir_ocaso = vec3(0.6, 0.0, -0.78);
uniform float brillo_estrellas = 1.6;

// --- Nubes ---
uniform sampler2D ruido_nubes : filter_linear_mipmap, repeat_enable;
uniform float cobertura : hint_range(0.0, 1.0) = 0.48;
uniform float suavidad : hint_range(0.01, 0.5) = 0.18;
uniform float escala_nubes = 0.55;
uniform vec3 nube_luz : source_color = vec3(1.0, 0.74, 0.52);
uniform vec3 nube_sombra : source_color = vec3(0.42, 0.34, 0.44);
uniform vec3 nube_noche : source_color = vec3(0.07, 0.075, 0.11);
uniform float brillo_nubes = 1.15;

// --- Sol / luna (sigue a LIGHT0) ---
uniform float radio_sol_grados = 1.4;
uniform float radio_luna_grados = 0.9;
uniform float energia_sol = 9.0;
uniform vec3 color_luna : source_color = vec3(0.9, 0.93, 1.0);
uniform float energia_luna = 3.0;
uniform float halo = 0.55;

vec3 hash33(vec3 p) {
	p = fract(p * vec3(0.1031, 0.1030, 0.0973));
	p += dot(p, p.yxz + 33.33);
	return fract((p.xxy + p.yxx) * p.zyx);
}

// Una capa de estrellas: una (o ninguna) por celda 3D sobre la esfera.
float capa_estrellas(vec3 dir, float densidad, float probabilidad, float tam) {
	vec3 p = dir * densidad;
	vec3 c = floor(p);
	vec3 r = hash33(c);
	float existe = step(1.0 - probabilidad, r.x);
	vec3 pos = c + 0.2 + 0.6 * hash33(c + 17.0);
	float d = length(p - pos);
	float brillo = 0.35 + 1.4 * pow(r.y, 5.0);
	return existe * brillo * (1.0 - smoothstep(0.0, tam * (0.7 + 0.6 * r.z), d));
}

float estrella_fija(vec3 dir, vec3 pos, float radio) {
	float d = acos(clamp(dot(dir, normalize(pos)), -1.0, 1.0));
	return 1.0 - smoothstep(radio * 0.3, radio, d);
}

void sky() {
	vec3 dir = normalize(EYEDIR);
	float h = dir.y;
	float n = clamp(noche, 0.0, 1.0);

	// Gradiente día / noche
	vec3 cielo;
	if (h >= 0.0) {
		float t = pow(clamp(h, 0.0, 1.0), curva);
		vec3 dia = t < 0.5 ? mix(color_horizonte, color_medio, t * 2.0) : mix(color_medio, color_cenit, t * 2.0 - 1.0);
		vec3 nch = t < 0.5 ? mix(noche_horizonte, noche_medio, t * 2.0) : mix(noche_medio, noche_cenit, t * 2.0 - 1.0);
		cielo = mix(dia, nch, n);
	} else {
		float t = pow(clamp(-h * 5.0, 0.0, 1.0), 0.6);
		cielo = mix(mix(color_suelo_horizonte, color_suelo, t), noche_suelo, n);
	}

	// Resplandor del crepúsculo hacia donde se puso el sol
	vec2 hor = normalize(dir.xz + 1e-4);
	vec2 hor_ocaso = normalize(dir_ocaso.xz + 1e-4);
	float hacia_ocaso = max(dot(hor, hor_ocaso), 0.0);
	float banda = exp(-abs(h) * 9.0);
	cielo += color_ocaso * crepusculo * pow(hacia_ocaso, 3.0) * banda * 0.9;

	vec3 dir_luz = vec3(0.0, 1.0, 0.0);
	vec3 color_luz = vec3(1.0, 0.8, 0.6);
	if (LIGHT0_ENABLED) {
		dir_luz = LIGHT0_DIRECTION;
		color_luz = LIGHT0_COLOR;
	}
	float cos_luz = dot(dir, dir_luz);

	// Halo del sol (se apaga de noche)
	float brillo_halo = pow(max(cos_luz, 0.0), 12.0) * halo + pow(max(cos_luz, 0.0), 180.0) * 1.5;
	cielo += color_luz * brillo_halo * (1.0 - clamp(h, 0.0, 1.0) * 0.6) * (1.0 - n);

	// Estrellas, Vía Láctea y Tres Marías (solo fondo, solo con cielo oscuro)
	float visibilidad = smoothstep(0.35, 0.9, n) * smoothstep(-0.02, 0.12, h);
	if (!AT_CUBEMAP_PASS && visibilidad > 0.0) {
		float s = capa_estrellas(dir, 70.0, 0.22, 0.16) * 1.5
			+ capa_estrellas(dir, 160.0, 0.30, 0.13) * 0.7
			+ capa_estrellas(dir, 300.0, 0.35, 0.12) * 0.35;
		// Vía Láctea: banda difusa sobre un gran círculo
		vec3 normal_via = normalize(vec3(0.35, 0.45, 0.82));
		float dv = dot(dir, normal_via);
		float via = exp(-dv * dv * 28.0);
		float textura = texture(ruido_nubes, vec2(atan(dir.z, dir.x) * 0.9, dir.y * 1.7)).r;
		vec3 color_via = vec3(0.55, 0.6, 0.78) * via * (0.35 + 0.65 * textura) * 0.22;
		s += via * capa_estrellas(dir, 420.0, 0.45, 0.12) * 0.5;
		// Tres Marías (cinturón de Orión), bajas hacia el este
		vec3 centro = normalize(vec3(-0.78, 0.42, -0.46));
		vec3 eje = normalize(cross(centro, vec3(0.0, 1.0, 0.0)) * 0.8 + vec3(0.0, 0.6, 0.0));
		float tm = estrella_fija(dir, centro - eje * 0.026, 0.0035)
			+ estrella_fija(dir, centro, 0.0038)
			+ estrella_fija(dir, centro + eje * 0.027, 0.0035);
		cielo += (vec3(0.9, 0.93, 1.0) * (s + tm * 2.2) * brillo_estrellas + color_via) * visibilidad;
	}

	// Nubes proyectadas sobre un plano alto
	if (h > 0.0) {
		vec2 uv = dir.xz / (h + 0.09) * escala_nubes;
		float n1 = texture(ruido_nubes, uv * 0.11 + vec2(0.13, 0.71)).r;
		float n2 = texture(ruido_nubes, uv * 0.37 + vec2(0.57, 0.29)).r;
		float nn = n1 * 0.68 + n2 * 0.32;
		float cob = cobertura * (1.0 - 0.25 * n);
		float densidad = smoothstep(1.0 - cob, 1.0 - cob + suavidad, nn);
		densidad *= smoothstep(0.0, 0.1, h);
		vec2 hor_luz = normalize(dir_luz.xz + 1e-4);
		float frente = dot(hor, hor_luz) * 0.5 + 0.5;
		float borde = 1.0 - smoothstep(0.0, 0.35, nn - (1.0 - cob));
		vec3 col_dia = mix(nube_sombra, nube_luz * color_luz, clamp(frente * 0.8 + borde * 0.5, 0.0, 1.0));
		col_dia *= brillo_nubes * mix(1.0, 0.75, clamp(h * 2.0, 0.0, 1.0));
		// de noche las nubes quedan oscuras, con el borde apenas plateado por la luna
		vec3 col_noche = nube_noche + color_luna * 0.05 * borde * frente;
		vec3 col = mix(col_dia, col_noche, n);
		col += color_ocaso * crepusculo * pow(hacia_ocaso, 2.0) * 0.5 * (1.0 - n * 0.5);
		cielo = mix(cielo, col, densidad * 0.92);
		color_luz *= 1.0 - densidad * 0.85;
	}

	// Disco del sol de día, de la luna de noche (ambos siguen a LIGHT0)
	float radio = radians(mix(radio_sol_grados, radio_luna_grados, step(0.5, n)));
	float ang = acos(clamp(cos_luz, -1.0, 1.0));
	float disco = 1.0 - smoothstep(radio * 0.85, radio, ang);
	vec3 color_disco = mix(color_luz * energia_sol, color_luna * energia_luna, step(0.5, n));
	float halo_luna = pow(max(cos_luz, 0.0), 400.0) * 0.25 * step(0.5, n);
	cielo += color_disco * disco + color_luna * halo_luna;

	COLOR = cielo;
}
