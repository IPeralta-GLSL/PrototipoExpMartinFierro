//! El gaucho narrador: recita el guion (versos de Martín Fierro) con voz
//! generada por Qwen3-TTS, muestra subtítulos y dispara los eventos de la
//! escena (anochecer, pasar de escena por el fuego...). Todo corre por tiempo:
//! no espera ninguna acción del jugador.
//!
//! Guion (JSON): lista de líneas
//! `{ "texto": "...", "audio": "res://voz/x.ogg", "espera": 2.0, "evento": "anochecer",
//!    "accion": "saludar", "mirar": "jugador", "ir_a": "PuntoCharla" }`
//! - `accion`: animación mientras habla (por defecto "hablar").
//! - `mirar`: "jugador" (por defecto) o el nombre de un nodo hermano del narrador.
//! - `ir_a`: nodo hermano hasta el que camina antes de decir la línea.
//! - `postura`: "sentado" (se sienta; usa las variantes "<accion>_sentado") o "parado".
//!
//! Si falta el audio, la línea dura según el largo del texto (solo subtítulo).

use godot::classes::{
    audio_stream_player_3d::AttenuationModel, base_material_3d::BillboardMode, AudioStream,
    AudioStreamPlayer3D, Engine, FileAccess, Input, Json, Label3D, ResourceLoader, SystemFont,
};
use godot::global::Key;
use godot::prelude::*;

use crate::ciclo::CicloDia;
use crate::guitarra::GuitarraCriolla;
use crate::modelos::ModeloIA;
use crate::personajes::{altura_global, girar_hacia, Evitador, Mandibula};
use crate::portal::PortalFogon;
use crate::terreno::TerrenoPampa;

struct Linea {
    texto: String,
    audio: String,
    espera: f32,
    evento: String,
    accion: String,
    mirar: String,
    ir_a: String,
    postura: String,
}

#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct Narrador {
    /// Ruta del guion JSON (res://guion/....json).
    #[export]
    guion: GString,
    /// El gaucho (ModeloIA); si tiene animaciones, actúa lo que dice.
    #[export]
    modelo: Option<Gd<ModeloIA>>,
    #[export]
    ciclo: Option<Gd<CicloDia>>,
    #[export]
    portal: Option<Gd<PortalFogon>>,
    #[export]
    terreno: Option<Gd<TerrenoPampa>>,
    /// Guitarra que rasguea mientras canta (evento "guitarra" del guion).
    #[export]
    guitarra: Option<Gd<GuitarraCriolla>>,
    /// Segundos antes de la primera línea.
    #[export]
    demora_inicial: f32,
    /// Qué hace antes de empezar a hablar (p. ej. "asar").
    #[export]
    accion_inicial: GString,
    /// A qué mira antes de empezar ("" = no gira; "jugador" o un nodo hermano).
    #[export]
    mirar_inicial: GString,
    /// Velocidad al caminar (m/s).
    #[export]
    velocidad: f32,
    /// Altura del renglón de abajo del subtítulo, con el narrador parado (m).
    #[export]
    altura_subtitulo: f32,
    #[export]
    volumen_db: f32,

    lineas: Vec<Linea>,
    indice: usize,
    esperando: f32,
    hablando: bool,
    t_linea: f32,
    dur_linea: f32,
    terminado: bool,
    tecla_previa: bool,
    mirada: String,
    destino: Option<Vector3>,
    evitador: Evitador,
    sentado: bool,
    /// Renglones del subtítulo (una "página" de hasta RENGLONES a la vez).
    renglones: Vec<Gd<Label3D>>,
    /// Texto partido en renglones con el segundo en que empieza cada uno.
    filas: Vec<(String, f32)>,
    /// Páginas como rangos de `filas`.
    paginas: Vec<(usize, usize)>,
    pagina_mostrada: Option<usize>,
    /// Mandíbula del gaucho (se abre con el volumen de la voz).
    boca: Option<Gd<Mandibula>>,
    envolvente: Vec<f32>,
    apertura: f32,
    /// Segundos que le quedan al rasguido de milonga.
    rasgueo: f32,
    t_compas: f32,
    compas: usize,
    golpe: usize,
    voz: Option<Gd<AudioStreamPlayer3D>>,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for Narrador {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            guion: GString::new(),
            modelo: None,
            ciclo: None,
            portal: None,
            terreno: None,
            guitarra: None,
            demora_inicial: 4.0,
            accion_inicial: GString::from("idle"),
            mirar_inicial: GString::new(),
            velocidad: 1.0,
            altura_subtitulo: 2.12,
            volumen_db: 4.0,
            lineas: Vec::new(),
            indice: 0,
            esperando: 0.0,
            hablando: false,
            t_linea: 0.0,
            dur_linea: 0.0,
            terminado: true,
            tecla_previa: false,
            mirada: String::new(),
            destino: None,
            evitador: Evitador::nuevo(),
            sentado: false,
            renglones: Vec::new(),
            filas: Vec::new(),
            paginas: Vec::new(),
            pagina_mostrada: None,
            boca: None,
            envolvente: Vec::new(),
            apertura: 0.0,
            rasgueo: 0.0,
            t_compas: 0.0,
            compas: 0,
            golpe: 0,
            voz: None,
            base,
        }
    }

    fn ready(&mut self) {
        if Engine::singleton().is_editor_hint() {
            return;
        }
        self.base_mut().add_to_group("narrador");
        self.lineas = cargar_guion(&self.guion.to_string());
        if self.lineas.is_empty() {
            godot_warn!("Narrador: guion vacío o no encontrado: {}", self.guion);
            return;
        }

        // Subtítulo flotante sobre el gaucho (gira solo en Y para leerse de frente).
        let mut fuente = SystemFont::new_gd();
        fuente.set_font_names(&PackedStringArray::from(&[
            GString::from("Georgia"),
            GString::from("Times New Roman"),
            GString::from("Noto Serif"),
            GString::from("serif"),
        ]));
        // Un Label3D por renglón: se apilan hacia arriba desde encima del sombrero.
        for i in 0..RENGLONES {
            let mut sub = Label3D::new_alloc();
            sub.set_name(&format!("Subtitulo{i}"));
            sub.set_font(&fuente);
            sub.set_font_size(46);
            sub.set_pixel_size(0.003);
            sub.set_outline_size(12);
            sub.set_modulate(Color::from_rgba(1.0, 0.94, 0.84, 0.0));
            sub.set_outline_modulate(Color::from_rgba(0.08, 0.04, 0.02, 0.0));
            sub.set_billboard_mode(BillboardMode::FIXED_Y);
            sub.set_position(Vector3::new(0.0, self.altura_subtitulo, 0.0));
            sub.set_visible(false);
            self.base_mut().add_child(&sub);
            self.renglones.push(sub);
        }

        let mut voz = AudioStreamPlayer3D::new_alloc();
        voz.set_name("Voz");
        voz.set_unit_size(4.0);
        voz.set_max_distance(45.0);
        voz.set_attenuation_model(AttenuationModel::INVERSE_DISTANCE);
        voz.set_volume_db(self.volumen_db);
        voz.set_position(Vector3::new(0.0, 1.6, 0.0));
        self.base_mut().add_child(&voz);
        self.voz = Some(voz);

        // Boca: si el modelo animado tiene mandíbula, se mueve con la voz.
        let esqueleto = self.modelo.as_ref().and_then(|m| m.bind().esqueleto());
        if let Some(mut sk) = esqueleto {
            if sk.find_bone("mandibula") >= 0 {
                let mut boca = Mandibula::new_alloc();
                boca.set_name("Boca");
                sk.add_child(&boca);
                self.boca = Some(boca);
            }
        }

        self.indice = 0;
        self.esperando = self.demora_inicial;
        self.terminado = false;
        self.mirada = self.mirar_inicial.to_string();
        let a = self.accion_inicial.to_string();
        self.animar(&format!("{a}|idle"));
    }

    fn process(&mut self, delta: f64) {
        if self.lineas.is_empty() {
            return;
        }
        let dt = delta as f32;

        // Caminando hacia un punto antes de decir la próxima línea.
        if let Some(destino) = self.destino {
            if self.caminar_hacia(destino, dt) {
                self.destino = None;
                self.comenzar_linea();
            }
            return;
        }
        self.orientar(dt);
        self.milonga(dt);
        if self.terminado {
            return;
        }

        // N = saltar la línea actual (para probar en la PC).
        let tecla = Input::singleton().is_key_pressed(Key::N);
        let saltar = tecla && !self.tecla_previa;
        self.tecla_previa = tecla;

        if !self.hablando {
            self.esperando -= dt;
            if self.esperando <= 0.0 || saltar {
                if let Some(d) = self.destino_de_linea() {
                    self.destino = Some(d);
                    self.animar("caminar|walk|idle");
                } else {
                    self.comenzar_linea();
                }
            }
            return;
        }

        self.t_linea += dt;
        self.mover_boca(dt);
        let total = self.dur_linea + 0.9;
        self.animar_subtitulos(total);
        if self.t_linea >= total || saltar {
            for r in self.renglones.iter_mut() {
                r.set_visible(false);
            }
            if saltar {
                if let Some(v) = self.voz.as_mut() {
                    v.stop();
                }
            }
            self.hablando = false;
            self.fijar_boca(0.0);
            let reposo = if self.sentado { "sentado|idle" } else { "idle|breath|stand" };
            self.animar(reposo);
            self.indice += 1;
            match self.lineas.get(self.indice) {
                Some(l) => self.esperando = l.espera,
                None => self.terminado = true,
            }
        }
    }
}

#[godot_api]
impl Narrador {
    /// true mientras quedan líneas por decir.
    #[func]
    pub fn narrando(&self) -> bool {
        !self.terminado
    }
}

impl Narrador {
    fn comenzar_linea(&mut self) {
        let Some(l) = self.lineas.get(self.indice) else {
            self.terminado = true;
            return;
        };
        let texto = l.texto.clone();
        let audio = l.audio.clone();
        let evento = l.evento.clone();
        let accion = if l.accion.is_empty() { "hablar".to_string() } else { l.accion.clone() };
        match l.postura.as_str() {
            "sentado" => self.sentado = true,
            "parado" => self.sentado = false,
            _ => {}
        }
        self.mirada = if l.mirar.is_empty() { "jugador".to_string() } else { l.mirar.clone() };

        let mut duracion = 1.2 + texto.chars().count() as f32 * 0.062;
        if !audio.is_empty() && ResourceLoader::singleton().exists(&audio) {
            if let Ok(stream) = try_load::<AudioStream>(&audio) {
                let largo = stream.get_length() as f32;
                if largo > 0.1 {
                    duracion = largo;
                }
                if let Some(v) = self.voz.as_mut() {
                    v.set_stream(&stream);
                    v.play();
                }
            }
        }
        self.dur_linea = duracion;
        self.preparar_subtitulos(&texto, duracion);
        self.envolvente = cargar_envolvente(&audio);
        godot_print!(
            "Narrador: línea {} de {} ({:.1} s) acción: {}{}",
            self.indice + 1,
            self.lineas.len(),
            duracion,
            accion,
            if evento.is_empty() { String::new() } else { format!(" evento: {evento}") }
        );
        self.t_linea = 0.0;
        self.hablando = true;
        if self.sentado {
            self.animar(&format!("{accion}_sentado|hablar_sentado|sentado"));
        } else {
            self.animar(&format!("{accion}|hablar|talk|idle"));
        }
        self.disparar(&evento, duracion);
    }

    /// Parte el texto en renglones (los largos, en dos) y les asigna el segundo en
    /// que la voz llega a cada uno (proporcional a las letras), agrupados en páginas.
    fn preparar_subtitulos(&mut self, texto: &str, duracion: f32) {
        let mut renglones: Vec<String> = Vec::new();
        for linea in texto.lines().map(str::trim).filter(|l| !l.is_empty()) {
            renglones.extend(partir_renglon(linea, 38));
        }
        let letras: Vec<usize> = renglones.iter().map(|r| r.chars().count().max(1)).collect();
        let total: usize = letras.iter().sum::<usize>().max(1);
        // La voz arranca un poco después y termina antes del final del audio.
        let (inicio, fin) = (0.15_f32, (duracion - 0.3).max(0.5));
        let mut acumulado = 0usize;
        self.filas.clear();
        for (r, n) in renglones.into_iter().zip(letras) {
            let t = if acumulado == 0 {
                0.0
            } else {
                inicio + (fin - inicio) * acumulado as f32 / total as f32
            };
            self.filas.push((r, t));
            acumulado += n;
        }
        // Páginas parejas de hasta RENGLONES (6 versos: 3 + 3; 4: 2 + 2).
        let n = self.filas.len();
        let cant = n.div_ceil(RENGLONES).max(1);
        let por_pagina = n.div_ceil(cant).max(1);
        self.paginas = (0..n).step_by(por_pagina).map(|a| (a, (a + por_pagina).min(n))).collect();
        self.pagina_mostrada = None;
    }

    /// Cada renglón entra con un fundido y sube un poco; al cambiar de página, la
    /// anterior se desvanece hacia arriba. Encima del sombrero (más bajo si está sentado).
    fn animar_subtitulos(&mut self, total: f32) {
        if self.paginas.is_empty() {
            return;
        }
        let t = self.t_linea;
        let mut p = 0;
        for (i, &(a, _)) in self.paginas.iter().enumerate() {
            if self.filas[a].1 <= t {
                p = i;
            }
        }
        let (a, b) = self.paginas[p];
        if self.pagina_mostrada != Some(p) {
            self.pagina_mostrada = Some(p);
            for (k, r) in self.renglones.iter_mut().enumerate() {
                if a + k < b {
                    r.set_text(self.filas[a + k].0.as_str());
                    r.set_visible(true);
                } else {
                    r.set_visible(false);
                }
            }
        }
        let fin_pagina = match self.paginas.get(p + 1) {
            Some(&(sig, _)) => self.filas[sig].1,
            None => total,
        };
        let salida = ((fin_pagina - t) / 0.4).clamp(0.0, 1.0);
        let base = self.altura_subtitulo - if self.sentado { 0.45 } else { 0.0 };
        let n = b - a;
        for k in 0..n.min(self.renglones.len()) {
            let e = ((t - self.filas[a + k].1) / 0.4).clamp(0.0, 1.0);
            let suave = 1.0 - (1.0 - e).powi(3);
            let alfa = suave * salida * salida;
            let y = base + (n - 1 - k) as f32 * INTERLINEADO - 0.07 * (1.0 - suave)
                + 0.08 * (1.0 - salida);
            let r = &mut self.renglones[k];
            r.set_position(Vector3::new(0.0, y, 0.0));
            r.set_modulate(Color::from_rgba(1.0, 0.94, 0.84, alfa));
            r.set_outline_modulate(Color::from_rgba(0.08, 0.04, 0.02, alfa * 0.9));
        }
    }

    /// Posición global de un nodo hermano del narrador (o del jugador).
    fn posicion_de(&self, nombre: &str) -> Option<Vector3> {
        if nombre.is_empty() {
            return None;
        }
        if nombre == "jugador" {
            return self
                .base()
                .get_viewport()
                .and_then(|v| v.get_camera_3d())
                .map(|c| c.get_global_position());
        }
        let padre = self.base().get_parent()?;
        let n = padre.get_node_or_null(nombre)?;
        n.try_cast::<Node3D>().ok().map(|n| n.get_global_position())
    }

    fn destino_de_linea(&self) -> Option<Vector3> {
        let l = self.lineas.get(self.indice)?;
        let d = self.posicion_de(&l.ir_a)?;
        let p = self.base().get_global_position();
        (Vector2::new(d.x - p.x, d.z - p.z).length() > 0.08).then_some(d)
    }

    /// Gira hacia lo que está mirando (jugador, asado, fuego...).
    fn orientar(&mut self, dt: f32) {
        let mirada = self.mirada.clone();
        if let Some(obj) = self.posicion_de(&mirada) {
            let mut nodo = self.base().clone().upcast::<Node3D>();
            girar_hacia(&mut nodo, obj, 2.2, dt);
        }
    }

    /// Camina hacia `destino`; devuelve true al llegar.
    fn caminar_hacia(&mut self, destino: Vector3, dt: f32) -> bool {
        let mut nodo = self.base().clone().upcast::<Node3D>();
        let (dir, trabado) = self.evitador.paso(&nodo, destino, 0.3, dt);
        let mut pos = nodo.get_global_position();
        let dist = Vector2::new(destino.x - pos.x, destino.z - pos.z).length();
        if dist < 0.05 || trabado {
            self.evitador.reiniciar();
            return true;
        }
        // Rodea lo que tenga adelante (el asado, el fuego...).
        let falta = girar_hacia(&mut nodo, pos + Vector3::new(dir.x, 0.0, dir.y), 3.5, dt);
        let v = self.velocidad * if falta > 0.8 { 0.25 } else { 1.0 };
        let paso = (v * dt).min(dist);
        pos.x += dir.x * paso;
        pos.z += dir.y * paso;
        if let Some(h) = altura_global(&self.terreno, pos) {
            pos.y = h;
        }
        nodo.set_global_position(pos);
        false
    }

    /// `duracion`: lo que dura la línea que dispara el evento.
    fn disparar(&mut self, evento: &str, duracion: f32) {
        for e in evento.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            match e {
                "anochecer" => {
                    if let Some(c) = self.ciclo.as_mut() {
                        c.bind_mut().iniciar_anochecer();
                    }
                }
                "noche" => {
                    if let Some(c) = self.ciclo.as_mut() {
                        c.bind_mut().fijar_progreso(1.0);
                    }
                }
                "guitarra" => {
                    // Rasguea durante la línea y un poco después.
                    if self.rasgueo <= 0.0 {
                        self.t_compas = 0.0;
                        self.golpe = 0;
                    }
                    self.rasgueo = duracion + 2.6;
                }
                "portal" => {
                    // El fuego se lleva al jugador apenas el gaucho termina de hablar.
                    if let Some(p) = self.portal.as_mut() {
                        p.bind_mut().activar_en(duracion + 1.2);
                    }
                }
                otro => {
                    godot_warn!("Narrador: evento desconocido '{otro}'");
                }
            }
        }
    }

    /// Abre la boca según el volumen de la voz en este instante (envolvente
    /// precalculada en res://voz/<línea>.boca.json, 30 valores por segundo).
    fn mover_boca(&mut self, dt: f32) {
        let t = self.t_linea;
        let objetivo = if t >= self.dur_linea {
            0.0
        } else if self.envolvente.is_empty() {
            // Sin envolvente: un movimiento genérico mientras dura la línea.
            (0.35 + 0.3 * (t * 17.0).sin() * (t * 5.3).sin()).clamp(0.0, 1.0)
        } else {
            self.envolvente.get((t * 30.0) as usize).copied().unwrap_or(0.0)
        };
        let k = (dt * 22.0).min(1.0);
        let a = self.apertura + (objetivo - self.apertura) * k;
        self.fijar_boca(a);
    }

    /// Milonga: compás de 2.4 s con rasguidos en 0, 0.9 y 1.8 s (3+3+2),
    /// Mi menor / Si séptima / La menor.
    fn milonga(&mut self, dt: f32) {
        const GOLPES: [(f32, f32); 3] = [(0.0, 0.62), (0.9, 0.42), (1.8, 0.47)];
        const ACORDES: [i32; 8] = [0, 0, 1, 1, 2, 0, 1, 0];
        const COMPAS: f32 = 2.4;
        if self.rasgueo <= 0.0 {
            return;
        }
        self.rasgueo -= dt;
        self.t_compas += dt;
        while self.golpe < GOLPES.len() && self.t_compas >= GOLPES[self.golpe].0 {
            let (_, fuerza) = GOLPES[self.golpe];
            let acorde = ACORDES[self.compas % ACORDES.len()];
            if let Some(g) = self.guitarra.as_mut() {
                g.bind_mut().rasguear_acorde(acorde, true, fuerza);
            }
            self.golpe += 1;
        }
        if self.t_compas >= COMPAS {
            self.t_compas -= COMPAS;
            self.compas += 1;
            self.golpe = 0;
        }
    }

    fn fijar_boca(&mut self, a: f32) {
        self.apertura = a;
        if let Some(b) = self.boca.as_mut() {
            b.bind_mut().apertura = a;
        }
    }

    fn animar(&mut self, candidatos: &str) {
        if let Some(m) = self.modelo.as_mut() {
            m.bind_mut().reproducir(GString::from(candidatos));
        }
    }
}

/// Envolvente de volumen de una línea (0..1, 30 por segundo), si existe.
/// Renglones por página de subtítulo y separación entre renglones (m).
const RENGLONES: usize = 3;
const INTERLINEADO: f32 = 0.165;

/// Corta un renglón largo en dos por el espacio más cercano a la mitad.
fn partir_renglon(linea: &str, max: usize) -> Vec<String> {
    let letras: Vec<char> = linea.chars().collect();
    if letras.len() <= max {
        return vec![linea.to_string()];
    }
    let medio = letras.len() / 2;
    let corte = (0..letras.len())
        .filter(|&i| letras[i] == ' ')
        .min_by_key(|&i| i.abs_diff(medio));
    match corte {
        Some(i) => vec![
            letras[..i].iter().collect::<String>().trim().to_string(),
            letras[i + 1..].iter().collect::<String>().trim().to_string(),
        ],
        None => vec![linea.to_string()],
    }
}

fn cargar_envolvente(audio: &str) -> Vec<f32> {
    let Some(base) = audio.strip_suffix(".ogg") else {
        return Vec::new();
    };
    let ruta = format!("{base}.boca.json");
    if !FileAccess::file_exists(&ruta) {
        return Vec::new();
    }
    let datos = Json::parse_string(&FileAccess::get_file_as_string(&ruta));
    datos
        .try_to::<VarArray>()
        .map(|a| a.iter_shared().filter_map(|v| v.try_to::<f64>().ok()).map(|v| v as f32).collect())
        .unwrap_or_default()
}

fn cargar_guion(ruta: &str) -> Vec<Linea> {
    if ruta.is_empty() || !FileAccess::file_exists(ruta) {
        return Vec::new();
    }
    let texto = FileAccess::get_file_as_string(ruta);
    let datos = Json::parse_string(&texto);
    let Ok(lista) = datos.try_to::<VarArray>() else {
        godot_error!("Narrador: el guion {ruta} no es una lista JSON");
        return Vec::new();
    };
    let mut lineas = Vec::new();
    for item in lista.iter_shared() {
        let Ok(d) = item.try_to::<VarDictionary>() else {
            continue;
        };
        let cadena = |k: &str| {
            d.get(k)
                .and_then(|v| v.try_to::<GString>().ok())
                .map(|s| s.to_string())
                .unwrap_or_default()
        };
        let espera = d
            .get("espera")
            .and_then(|v| v.try_to::<f64>().ok())
            .unwrap_or(1.5) as f32;
        lineas.push(Linea {
            texto: cadena("texto"),
            audio: cadena("audio"),
            espera,
            evento: cadena("evento"),
            accion: cadena("accion"),
            mirar: cadena("mirar"),
            ir_a: cadena("ir_a"),
            postura: cadena("postura"),
        });
    }
    lineas
}
