//! El gaucho narrador: recita el guion (versos de Martín Fierro) con voz
//! generada por Qwen3-TTS, muestra subtítulos y dispara los eventos de la
//! escena (anochecer, abrir el portal del fogón...).
//!
//! Guion (JSON): lista de líneas
//! `{ "texto": "...", "audio": "res://voz/x.ogg", "espera": 2.0, "evento": "anochecer" }`.
//! Si falta el audio, la línea dura según el largo del texto (solo subtítulo).

use godot::classes::{
    audio_stream_player_3d::AttenuationModel, base_material_3d::BillboardMode, AudioStream,
    AudioStreamPlayer3D, Engine, FileAccess, Input, Json, Label3D, ResourceLoader, SystemFont,
};
use godot::global::Key;
use godot::prelude::*;

use crate::ciclo::CicloDia;
use crate::modelos::ModeloIA;
use crate::portal::PortalFogon;

struct Linea {
    texto: String,
    audio: String,
    espera: f32,
    evento: String,
}

#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct Narrador {
    /// Ruta del guion JSON (res://guion/....json).
    #[export]
    guion: GString,
    /// El gaucho (ModeloIA); si tiene animaciones, alterna "hablar" / "idle".
    #[export]
    modelo: Option<Gd<ModeloIA>>,
    #[export]
    ciclo: Option<Gd<CicloDia>>,
    #[export]
    portal: Option<Gd<PortalFogon>>,
    /// Segundos antes de la primera línea.
    #[export]
    demora_inicial: f32,
    /// Altura del subtítulo sobre el narrador (m).
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
    subtitulo: Option<Gd<Label3D>>,
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
            demora_inicial: 4.0,
            altura_subtitulo: 2.2,
            volumen_db: 4.0,
            lineas: Vec::new(),
            indice: 0,
            esperando: 0.0,
            hablando: false,
            t_linea: 0.0,
            dur_linea: 0.0,
            terminado: true,
            tecla_previa: false,
            subtitulo: None,
            voz: None,
            base,
        }
    }

    fn ready(&mut self) {
        if Engine::singleton().is_editor_hint() {
            return;
        }
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
        let mut sub = Label3D::new_alloc();
        sub.set_name("Subtitulo");
        sub.set_font(&fuente);
        sub.set_font_size(48);
        sub.set_pixel_size(0.003);
        sub.set_outline_size(12);
        sub.set_modulate(Color::from_rgba(1.0, 0.94, 0.84, 0.0));
        sub.set_outline_modulate(Color::from_rgba(0.08, 0.04, 0.02, 0.0));
        sub.set_billboard_mode(BillboardMode::FIXED_Y);
        sub.set_position(Vector3::new(0.0, self.altura_subtitulo, 0.0));
        self.base_mut().add_child(&sub);
        self.subtitulo = Some(sub);

        let mut voz = AudioStreamPlayer3D::new_alloc();
        voz.set_name("Voz");
        voz.set_unit_size(4.0);
        voz.set_max_distance(45.0);
        voz.set_attenuation_model(AttenuationModel::INVERSE_DISTANCE);
        voz.set_volume_db(self.volumen_db);
        voz.set_position(Vector3::new(0.0, 1.6, 0.0));
        self.base_mut().add_child(&voz);
        self.voz = Some(voz);

        self.indice = 0;
        self.esperando = self.demora_inicial;
        self.terminado = false;
        self.animar("idle|breath|stand");
    }

    fn process(&mut self, delta: f64) {
        if self.terminado {
            return;
        }
        let dt = delta as f32;

        // N = saltar la línea actual (para probar en la PC).
        let tecla = Input::singleton().is_key_pressed(Key::N);
        let saltar = tecla && !self.tecla_previa;
        self.tecla_previa = tecla;

        if !self.hablando {
            self.esperando -= dt;
            if self.esperando <= 0.0 || saltar {
                self.comenzar_linea();
            }
            return;
        }

        self.t_linea += dt;
        let total = self.dur_linea + 0.9;
        let alfa = (self.t_linea / 0.5).min((total - self.t_linea) / 0.6).clamp(0.0, 1.0);
        if let Some(s) = self.subtitulo.as_mut() {
            let mut c = s.get_modulate();
            c.a = alfa;
            s.set_modulate(c);
            let mut o = s.get_outline_modulate();
            o.a = alfa * 0.9;
            s.set_outline_modulate(o);
        }
        if self.t_linea >= total || saltar {
            if saltar {
                if let Some(v) = self.voz.as_mut() {
                    v.stop();
                }
            }
            self.hablando = false;
            self.animar("idle|breath|stand");
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

        if let Some(s) = self.subtitulo.as_mut() {
            s.set_text(&texto);
        }
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
        godot_print!(
            "Narrador: línea {} de {} ({:.1} s){}",
            self.indice + 1,
            self.lineas.len(),
            duracion,
            if evento.is_empty() { String::new() } else { format!(" evento: {evento}") }
        );
        self.t_linea = 0.0;
        self.hablando = true;
        self.animar("talk|hablar|gesture|speak|idle");
        self.disparar(&evento);
    }

    fn disparar(&mut self, evento: &str) {
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
                "portal" => {
                    if let Some(p) = self.portal.as_mut() {
                        p.bind_mut().activar();
                    }
                }
                otro => {
                    godot_warn!("Narrador: evento desconocido '{otro}'");
                }
            }
        }
    }

    fn animar(&mut self, candidatos: &str) {
        if let Some(m) = self.modelo.as_mut() {
            m.bind_mut().reproducir(GString::from(candidatos));
        }
    }
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
        });
    }
    lineas
}
