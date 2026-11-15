// Cielo de atardecer pampeano: gradiente, sol con halo y cúmulos
// iluminados desde el lado del sol. Sin TIME: el cielo (y su luz ambiente)
// se calcula una sola vez, barato para el Quest.
shader_type sky;

uniform vec3 color_cenit : source_color = vec3(0.18, 0.29, 0.49);
uniform vec3 color_medio : source_color = vec3(0.62, 0.48, 0.58);
uniform vec3 color_horizonte : source_color = vec3(0.98, 0.64, 0.38);
uniform vec3 color_suelo_horizonte : source_color = vec3(0.78, 0.58, 0.37);
uniform vec3 color_suelo : source_color = vec3(0.29, 0.27, 0.2);
uniform float curva = 0.45;

uniform sampler2D ruido_nubes : filter_linear_mipmap, repeat_enable;
uniform float cobertura : hint_range(0.0, 1.0) = 0.48;
uniform float suavidad : hint_range(0.01, 0.5) = 0.18;
uniform float escala_nubes = 0.55;
uniform vec3 nube_luz : source_color = vec3(1.0, 0.74, 0.52);
uniform vec3 nube_sombra : source_color = vec3(0.42, 0.34, 0.44);
uniform float brillo_nubes = 1.15;

uniform float radio_sol_grados = 1.4;
uniform float energia_sol = 9.0;
uniform float halo = 0.55;

void sky() {
	vec3 dir = normalize(EYEDIR);
	float h = dir.y;
	vec3 cielo;
	if (h >= 0.0) {
		float t = pow(clamp(h, 0.0, 1.0), curva);
		cielo = t < 0.5
			? mix(color_horizonte, color_medio, t * 2.0)
			: mix(color_medio, color_cenit, t * 2.0 - 1.0);
	} else {
		float t = pow(clamp(-h * 5.0, 0.0, 1.0), 0.6);
		cielo = mix(color_suelo_horizonte, color_suelo, t);
	}

	vec3 dir_sol = vec3(0.0, 1.0, 0.0);
	vec3 color_sol = vec3(1.0, 0.8, 0.6);
	if (LIGHT0_ENABLED) {
		dir_sol = LIGHT0_DIRECTION;
		color_sol = LIGHT0_COLOR;
	}
	float cos_sol = dot(dir, dir_sol);

	// halo cálido alrededor del sol, más fuerte cerca del horizonte
	float brillo_halo = pow(max(cos_sol, 0.0), 12.0) * halo + pow(max(cos_sol, 0.0), 180.0) * 1.5;
	cielo += color_sol * brillo_halo * (1.0 - clamp(h, 0.0, 1.0) * 0.6);

	// nubes proyectadas sobre un plano alto
	if (h > 0.0) {
		vec2 uv = dir.xz / (h + 0.09) * escala_nubes;
		float n1 = texture(ruido_nubes, uv * 0.11 + vec2(0.13, 0.71)).r;
		float n2 = texture(ruido_nubes, uv * 0.37 + vec2(0.57, 0.29)).r;
		float n = n1 * 0.68 + n2 * 0.32;
		float densidad = smoothstep(1.0 - cobertura, 1.0 - cobertura + suavidad, n);
		densidad *= smoothstep(0.0, 0.1, h);
		// lado hacia el sol iluminado; espesor oscurece el centro
		vec2 hor = normalize(dir.xz + 1e-4);
		vec2 hor_sol = normalize(dir_sol.xz + 1e-4);
		float frente = dot(hor, hor_sol) * 0.5 + 0.5;
		float borde = 1.0 - smoothstep(0.0, 0.35, n - (1.0 - cobertura));
		vec3 col = mix(nube_sombra, nube_luz * color_sol, clamp(frente * 0.8 + borde * 0.5, 0.0, 1.0));
		col *= brillo_nubes * mix(1.0, 0.75, clamp(h * 2.0, 0.0, 1.0));
		cielo = mix(cielo, col, densidad * 0.92);
		// las nubes tapan el disco solar
		color_sol *= 1.0 - densidad * 0.85;
	}

	// disco solar
	float radio = radians(radio_sol_grados);
	float ang = acos(clamp(cos_sol, -1.0, 1.0));
	float disco = 1.0 - smoothstep(radio * 0.85, radio, ang);
	cielo += color_sol * energia_sol * disco;

	COLOR = cielo;
}
