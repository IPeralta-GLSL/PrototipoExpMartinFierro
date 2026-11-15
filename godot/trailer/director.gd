extends Node
## Director del teaser: mueve la cámara por planos, muestra los versos y toca
## una milonga con la guitarra sintetizada en Rust (GuitarraCriolla).
## Se renderiza cuadro a cuadro con el Movie Maker de Godot: tools/trailer.ps1.

@export var camara: Camera3D
@export var banda: Node
@export var fundido: ColorRect
@export var texto: Label
@export var titulo: Label
@export var subtitulo: Label
@export var creditos: Label
@export var bpm: float = 92.0

# Planos: duración + recorrido de la cámara (posición y punto de mira, inicio y fin),
# o una órbita alrededor de un centro.
const PLANOS := [
	{ "dur": 6.0, "fov": 55.0,  # horizonte al atardecer, el campamento a lo lejos
		"desde": Vector3(-22, 1.0, 20), "mirar_desde": Vector3(8, 3.0, -35),
		"hasta": Vector3(-17, 1.35, 14.5), "mirar_hasta": Vector3(4, 2.6, -30) },
	{ "dur": 5.5, "fov": 50.0,  # empuje bajo hacia el fogón
		"desde": Vector3(-1.2, 0.65, 7.5), "mirar_desde": Vector3(0, 0.6, 0),
		"hasta": Vector3(-0.5, 0.5, 3.4), "mirar_hasta": Vector3(0, 0.45, -0.2) },
	{ "dur": 6.0, "fov": 48.0, "orbita": true,  # alrededor del fuego
		"centro": Vector3(0, 0.35, 0), "radio": 3.0, "alto": 1.05,
		"ang_desde": -50.0, "ang_hasta": 25.0 },
	{ "dur": 5.0, "fov": 36.0,  # la vigüela y el sombrero
		"desde": Vector3(-0.8, 0.52, 2.95), "mirar_desde": Vector3(-1.45, 0.42, 2.0),
		"hasta": Vector3(-0.98, 0.46, 2.72), "mirar_hasta": Vector3(-1.55, 0.38, 1.95) },
	{ "dur": 5.0, "fov": 45.0,  # los caballos en el palenque
		"desde": Vector3(2.8, 1.5, -2.2), "mirar_desde": Vector3(5.8, 1.2, -5.2),
		"hasta": Vector3(4.0, 1.4, -3.4), "mirar_hasta": Vector3(6.4, 1.15, -6.3) },
	{ "dur": 4.0, "fov": 50.0,  # el rancho con el farol
		"desde": Vector3(0.3, 1.55, -3.6), "mirar_desde": Vector3(1.5, 1.3, -10.6),
		"hasta": Vector3(1.1, 1.45, -6.2), "mirar_hasta": Vector3(1.5, 1.2, -10.6) },
	{ "dur": 8.5, "fov": 55.0,  # grúa: se revela toda la pampa
		"desde": Vector3(-2.5, 1.6, 5.5), "mirar_desde": Vector3(0, 1.0, -5),
		"hasta": Vector3(-16, 10, 22), "mirar_hasta": Vector3(0, 2.5, -10) },
]
# Opciones (desde la línea de comandos: -- --sin-carteles --sin-creditos)
@export var mostrar_carteles := true
@export var mostrar_creditos := true
var negro_final := 3.2

# [inicio, fin, etiqueta, texto]
const TEXTOS := [
	[1.6, 5.4, "texto", "Pampa bonaerense, hacia 1872"],
	[6.8, 10.9, "texto", "Aquí me pongo á cantar"],
	[12.2, 16.9, "texto", "Al compás de la vigüela,"],
	[18.0, 22.0, "texto", "Que el hombre que lo desvela\nUna pena estrordinaria,"],
	[23.2, 27.0, "texto", "Como la ave solitaria"],
	[28.0, 31.2, "texto", "Con el cantar se consuela."],
	[33.0, 39.6, "titulo", "MARTÍN FIERRO"],
	[34.0, 39.6, "subtitulo", "Un fogón en la pampa  ·  Experiencia VR para Meta Quest 3"],
	[40.3, 42.9, "creditos", "Versos: José Hernández, El Gaucho Martín Fierro (1872)\nModelos 3D generados en local con Hunyuan3D-2 y Z-Image-Turbo\nHecho en Godot 4.7 + Rust"],
]

var t := 0.0
var _inicios: Array[float] = []
var _total := 0.0
var _golpes: Array = []  # [tiempo, acorde, abajo, fuerza]
var _proximo := 0


func _ready() -> void:
	camara.make_current()
	var args := OS.get_cmdline_user_args()
	if "--sin-carteles" in args:
		mostrar_carteles = false
	if "--sin-creditos" in args:
		mostrar_creditos = false
	var ocultar := ["../Pampa/Titulo", "../Pampa/PistaGuitarra"]
	if not mostrar_carteles:
		ocultar += ["../Pampa/Versos", "../Pampa/VersoLibre", "../Pampa/VersoHermanos"]
	if not mostrar_creditos:
		negro_final = 0.8
	# En el render no habla el narrador ni se abre el portal.
	for ruta in ["../Pampa/Campamento/Narrador", "../Pampa/Campamento/Fogon/Portal"]:
		var nodo := get_node_or_null(ruta)
		if nodo:
			nodo.process_mode = Node.PROCESS_MODE_DISABLED
	for ruta in ocultar:
		var n := get_node_or_null(ruta)
		if n:
			n.visible = false
	# Un poco más de brillo cinematográfico (solo en el teaser).
	var entorno := get_node_or_null("../Pampa/Entorno") as WorldEnvironment
	if entorno and entorno.environment:
		var env := entorno.environment
		env.glow_enabled = true
		env.glow_intensity = 0.55
		env.glow_bloom = 0.06
		env.glow_hdr_threshold = 1.1
	get_viewport().msaa_3d = Viewport.MSAA_4X

	var acum := 0.0
	for p in PLANOS:
		_inicios.append(acum)
		acum += p["dur"]
	_total = acum
	for l in [texto, titulo, subtitulo, creditos]:
		l.modulate.a = 0.0
	fundido.modulate.a = 1.0
	_programar_milonga()
	_mover_camara()


func _process(delta: float) -> void:
	t += delta
	_mover_camara()
	_fundidos()
	_textos()
	_musica()
	if t >= _total + negro_final:
		get_tree().quit()


func _suave(u: float) -> float:
	u = clampf(u, 0.0, 1.0)
	var s := u * u * (3.0 - 2.0 * u)
	return lerpf(u, s, 0.6)


func _plano_actual() -> int:
	for i in range(PLANOS.size() - 1, -1, -1):
		if t >= _inicios[i]:
			return i
	return 0


func _mover_camara() -> void:
	var i := _plano_actual()
	var p: Dictionary = PLANOS[i]
	var e := _suave((t - _inicios[i]) / p["dur"])
	var pos: Vector3
	var mira: Vector3
	if p.get("orbita", false):
		var a := deg_to_rad(lerpf(p["ang_desde"], p["ang_hasta"], e))
		var c: Vector3 = p["centro"]
		pos = c + Vector3(sin(a) * p["radio"], 0.0, cos(a) * p["radio"])
		pos.y = p["alto"]
		mira = c
	else:
		pos = (p["desde"] as Vector3).lerp(p["hasta"], e)
		mira = (p["mirar_desde"] as Vector3).lerp(p["mirar_hasta"], e)
	camara.global_position = pos
	camara.look_at(mira, Vector3.UP)
	camara.fov = p["fov"]


func _fundidos() -> void:
	var alfa := 0.0
	if t >= _total:
		alfa = 1.0
	else:
		var i := _plano_actual()
		var ini := _inicios[i]
		var fin := ini + float(PLANOS[i]["dur"])
		var entrada := 1.4 if i == 0 else 0.35
		var salida := 1.2 if i == PLANOS.size() - 1 else 0.3
		alfa = maxf(1.0 - clampf((t - ini) / entrada, 0.0, 1.0),
				clampf((t - (fin - salida)) / salida, 0.0, 1.0))
	fundido.modulate.a = alfa


func _textos() -> void:
	var etiquetas := {"texto": texto, "titulo": titulo, "subtitulo": subtitulo, "creditos": creditos}
	var alfas := {"texto": 0.0, "titulo": 0.0, "subtitulo": 0.0, "creditos": 0.0}
	for e in TEXTOS:
		if e[2] == "creditos" and not mostrar_creditos:
			continue
		var ini: float = e[0]
		var fin: float = e[1]
		if t >= ini and t <= fin:
			var a := minf(clampf((t - ini) / 0.6, 0.0, 1.0), clampf((fin - t) / 0.6, 0.0, 1.0))
			alfas[e[2]] = a
			var l: Label = etiquetas[e[2]]
			if l.text != e[3]:
				l.text = e[3]
	for k in etiquetas:
		etiquetas[k].modulate.a = alfas[k]


func _programar_milonga() -> void:
	# Milonga en 3+3+2: golpes en las corcheas 0, 3 y 6, más un repique arriba en la 7.
	var corchea := 30.0 / bpm
	var compas := corchea * 8.0
	var progresion := [0, 0, 1, 1, 0, 2, 1, 0]  # Mi m, Mi m, Si7, Si7, Mi m, La m, Si7, Mi m
	var patron := [[0, true, 0.95], [3, true, 0.7], [6, true, 0.75], [7, false, 0.4]]
	var inicio := 1.2
	var k := 0
	while inicio + k * compas < 32.4:
		var acorde: int = progresion[k % progresion.size()]
		for g in patron:
			_golpes.append([inicio + k * compas + g[0] * corchea, acorde, g[1], g[2]])
		k += 1
	# Acorde final con el título.
	_golpes.append([33.0, 0, true, 1.0])


func _musica() -> void:
	if banda == null:
		return
	while _proximo < _golpes.size() and t >= _golpes[_proximo][0]:
		var g: Array = _golpes[_proximo]
		banda.call("rasguear_acorde", g[1], g[2], g[3])
		_proximo += 1
