//! Jugador: origen XR con locomoción (stick izquierdo = caminar, stick derecho
//! = giro por pasos), seguimiento del relieve y límite del campamento.
//! Sin casco cae a modo escritorio (WASD + botón derecho del mouse para mirar)
//! y puede sacar capturas: `-- --captura=ruta.png --vista=x,alto,z,yaw,pitch`.

use godot::classes::{
    base_material_3d::{CullMode, Flags, ShadingMode, Transparency},
    display_server::VSyncMode,
    geometry_instance_3d::ShadowCastingSetting,
    viewport::VrsMode,
    xr_server::RotationMode,
    DisplayServer, Engine, IXrOrigin3D, Input, InputEvent, InputEventMouseMotion, Label3D,
    MeshInstance3D, OpenXrInterface, RenderingServer, SphereMesh, StandardMaterial3D, XrCamera3D,
    XrController3D, XrInterface, XrOrigin3D, XrServer,
};
use godot::global::{Key, MouseButton};
use godot::prelude::*;

use crate::conversion::argumento_usuario;
use crate::terreno::TerrenoPampa;

#[derive(GodotClass)]
#[class(base = XrOrigin3D)]
pub struct JugadorXR {
    #[export]
    terreno: Option<Gd<TerrenoPampa>>,
    /// Cartel de título que se desvanece después de unos segundos.
    #[export]
    titulo: Option<Gd<Label3D>>,
    /// Velocidad al caminar (m/s).
    #[export]
    velocidad: f32,
    #[export]
    giro_grados: f32,
    /// Distancia máxima al fogón (m).
    #[export]
    radio_limite: f32,
    /// Frecuencia de refresco deseada en el Quest 3 (72, 80, 90 o 120).
    #[export]
    frecuencia_hz: f32,
    #[export]
    altura_escritorio: f32,
    #[export]
    duracion_titulo: f32,
    /// Segundos de fundido desde negro al empezar la escena (0 = sin fundido).
    #[export]
    fundido_inicial: f32,

    modo_xr: bool,
    interfaz: Option<Gd<XrInterface>>,
    tasa_lista: bool,
    centrado: bool,
    cabeza: Option<Gd<XrCamera3D>>,
    mano_izq: Option<Gd<XrController3D>>,
    mano_der: Option<Gd<XrController3D>>,
    giro_listo: bool,
    yaw: f32,
    pitch: f32,
    tiempo: f32,
    captura: Option<String>,
    esperar_cuadros: u32,
    cuadros: u32,
    velo: Option<Gd<StandardMaterial3D>>,
    velo_externo: bool,
    base: Base<XrOrigin3D>,
}

#[godot_api]
impl IXrOrigin3D for JugadorXR {
    fn init(base: Base<XrOrigin3D>) -> Self {
        Self {
            terreno: None,
            titulo: None,
            velocidad: 1.6,
            giro_grados: 30.0,
            radio_limite: 40.0,
            frecuencia_hz: 90.0,
            altura_escritorio: 1.65,
            duracion_titulo: 9.0,
            fundido_inicial: 0.0,
            modo_xr: false,
            interfaz: None,
            tasa_lista: false,
            centrado: false,
            cabeza: None,
            mano_izq: None,
            mano_der: None,
            giro_listo: true,
            yaw: 0.0,
            pitch: 0.0,
            tiempo: 0.0,
            captura: None,
            esperar_cuadros: 90,
            cuadros: 0,
            velo: None,
            velo_externo: false,
            base,
        }
    }

    fn ready(&mut self) {
        self.cabeza = self.base().try_get_node_as::<XrCamera3D>("Cabeza");
        self.mano_izq = self.base().try_get_node_as::<XrController3D>("ManoIzquierda");
        self.mano_der = self.base().try_get_node_as::<XrController3D>("ManoDerecha");
        if Engine::singleton().is_editor_hint() {
            return;
        }
        self.base_mut().add_to_group("jugador");
        self.crear_velo();

        self.captura = argumento_usuario("captura").filter(|s| !s.is_empty());
        if let Some(n) = argumento_usuario("esperar").and_then(|s| s.parse::<u32>().ok()) {
            self.esperar_cuadros = n.max(1);
        }
        let escritorio = argumento_usuario("escritorio").is_some() || self.captura.is_some();
        self.modo_xr = !escritorio && self.iniciar_xr();
        if !self.modo_xr {
            self.iniciar_escritorio();
        } else {
            // Las manos del jugador (malla de hand tracking) y la interacción.
            let mut manos = crate::interaccion::ManosXR::new_alloc();
            manos.set_name("Manos");
            self.base_mut().add_child(&manos);
        }
        godot_print!(
            "Martín Fierro VR: modo {}",
            if self.modo_xr { "XR (casco)" } else { "escritorio" }
        );
    }

    fn process(&mut self, delta: f64) {
        let dt = (delta as f32).min(0.1);
        self.tiempo += dt;
        self.actualizar_titulo();
        if !self.velo_externo && self.fundido_inicial > 0.0 {
            let a = (1.0 - self.tiempo / self.fundido_inicial).clamp(0.0, 1.0);
            self.aplicar_velo(Color::from_rgba(0.0, 0.0, 0.0, a));
        }
        if self.modo_xr {
            self.ajustar_frecuencia();
            if !self.centrado && self.tiempo > 1.0 {
                // Mirar hacia el fogón al empezar, sin importar hacia dónde
                // estaba orientado el casco al abrir la app.
                XrServer::singleton().center_on_hmd(RotationMode::RESET_BUT_KEEP_TILT, true);
                self.centrado = true;
            }
            self.locomocion_xr(dt);
        } else {
            self.control_escritorio(dt);
        }
        self.tomar_captura();
    }

    fn unhandled_input(&mut self, evento: Gd<InputEvent>) {
        if self.modo_xr {
            return;
        }
        if let Ok(m) = evento.try_cast::<InputEventMouseMotion>() {
            if Input::singleton().is_mouse_button_pressed(MouseButton::RIGHT) {
                let rel = m.get_relative();
                self.yaw -= rel.x * 0.004;
                self.pitch = (self.pitch - rel.y * 0.004).clamp(-1.4, 1.4);
            }
        }
    }
}

#[godot_api]
impl JugadorXR {
    /// true si el jugador está usando el casco.
    #[func]
    pub fn en_modo_xr(&self) -> bool {
        self.modo_xr
    }

    /// Tiñe la vista (fundidos y transiciones). Alfa 0 = transparente.
    #[func]
    pub fn fijar_velo(&mut self, color: Color) {
        self.velo_externo = true;
        self.aplicar_velo(color);
    }

    /// Señal `pose_recentered` de OpenXR (botón Meta mantenido).
    #[func]
    fn al_recentrar(&mut self) {
        XrServer::singleton().center_on_hmd(RotationMode::RESET_BUT_KEEP_TILT, true);
    }
}

impl JugadorXR {
    /// Esfera pegada a la cámara, vista desde adentro, sin prueba de
    /// profundidad: tapa toda la vista en el casco y en la PC.
    fn crear_velo(&mut self) {
        let Some(mut cabeza) = self.cabeza.clone() else {
            return;
        };
        let mut esfera = SphereMesh::new_gd();
        esfera.set_radius(0.3);
        esfera.set_height(0.6);
        esfera.set_flip_faces(true);
        let mut mat = StandardMaterial3D::new_gd();
        mat.set_shading_mode(ShadingMode::UNSHADED);
        mat.set_transparency(Transparency::ALPHA);
        mat.set_cull_mode(CullMode::DISABLED);
        mat.set_flag(Flags::DISABLE_DEPTH_TEST, true);
        mat.set_flag(Flags::DISABLE_FOG, true);
        mat.set_render_priority(127);
        let inicial = if self.fundido_inicial > 0.0 { 1.0 } else { 0.0 };
        mat.set_albedo(Color::from_rgba(0.0, 0.0, 0.0, inicial));
        let mut mi = MeshInstance3D::new_alloc();
        mi.set_name("Velo");
        mi.set_mesh(&esfera);
        mi.set_material_override(&mat);
        mi.set_cast_shadows_setting(ShadowCastingSetting::OFF);
        mi.set_visible(inicial > 0.0);
        cabeza.add_child(&mi);
        self.velo = Some(mat);
    }

    fn aplicar_velo(&mut self, color: Color) {
        let Some(mat) = self.velo.as_mut() else {
            return;
        };
        mat.set_albedo(color);
        if let Some(mut mi) = self
            .cabeza
            .as_ref()
            .and_then(|c| c.try_get_node_as::<MeshInstance3D>("Velo"))
        {
            mi.set_visible(color.a > 0.002);
        }
    }

    fn iniciar_xr(&mut self) -> bool {
        let xr = XrServer::singleton();
        let Some(mut iface) = xr.find_interface("OpenXR") else {
            godot_print!("OpenXR no está disponible: modo escritorio.");
            return false;
        };
        if !iface.is_initialized() && !iface.initialize() {
            godot_print!("OpenXR no se pudo iniciar (¿casco desconectado?): modo escritorio.");
            return false;
        }
        if let Some(mut vp) = self.base().get_viewport() {
            vp.set_use_xr(true);
            // Con Vulkan (renderer Mobile), la foveación usa VRS en modo XR.
            if RenderingServer::singleton().get_rendering_device().is_some() {
                vp.set_vrs_mode(VrsMode::XR);
            }
        }
        // En XR el ritmo lo marca el casco, no el monitor.
        DisplayServer::singleton().window_set_vsync_mode(VSyncMode::DISABLED);
        // Mantener apretado el botón Meta recentra la vista: la escena acompaña.
        let yo = self.to_gd();
        iface.connect(
            "pose_recentered",
            &Callable::from_object_method(&yo, "al_recentrar"),
        );
        self.interfaz = Some(iface);
        true
    }

    /// Las frecuencias disponibles aparecen cuando la sesión OpenXR ya corre,
    /// por eso se reintenta en cada cuadro hasta lograrlo.
    fn ajustar_frecuencia(&mut self) {
        if self.tasa_lista {
            return;
        }
        let Some(iface) = self.interfaz.clone() else {
            self.tasa_lista = true;
            return;
        };
        let Ok(mut oxr) = iface.try_cast::<OpenXrInterface>() else {
            self.tasa_lista = true;
            return;
        };
        let tasas = oxr.get_available_display_refresh_rates();
        if tasas.is_empty() {
            if self.tiempo > 10.0 {
                self.tasa_lista = true;
            }
            return;
        }
        let objetivo = self.frecuencia_hz;
        let mut mejor: Option<f32> = None;
        for v in tasas.iter_shared() {
            if let Ok(f) = v.try_to::<f64>() {
                let f = f as f32;
                if f <= objetivo + 0.5 && mejor.is_none_or(|m| f > m) {
                    mejor = Some(f);
                }
            }
        }
        if let Some(f) = mejor {
            oxr.set_display_refresh_rate(f);
            godot_print!("Frecuencia de refresco: {f} Hz");
        }
        self.tasa_lista = true;
    }

    fn locomocion_xr(&mut self, dt: f32) {
        let (Some(cabeza), Some(izq), Some(der)) =
            (self.cabeza.clone(), self.mano_izq.clone(), self.mano_der.clone())
        else {
            return;
        };

        // Caminar con el stick izquierdo, en la dirección de la mirada.
        let mut stick = izq.get_vector2("primary");
        if stick.length() < 0.18 {
            stick = Vector2::ZERO;
        }
        if stick != Vector2::ZERO {
            let b = cabeza.get_global_transform().basis;
            let mut adelante = -b.col_c();
            adelante.y = 0.0;
            let mut derecha = b.col_a();
            derecha.y = 0.0;
            if adelante.length() > 1e-3 && derecha.length() > 1e-3 {
                let mov = (derecha.normalized() * stick.x + adelante.normalized() * stick.y)
                    * self.velocidad
                    * dt;
                let p = self.base().get_global_position();
                self.base_mut().set_global_position(p + mov);
            }
        }

        // Giro por pasos con el stick derecho, pivotando sobre la cabeza.
        let sx = der.get_vector2("primary").x;
        if sx.abs() > 0.7 && self.giro_listo {
            let ang = -sx.signum() * self.giro_grados.to_radians();
            let pivote = cabeza.get_global_position();
            let t = self.base().get_global_transform();
            let t2 = t
                .translated(-pivote)
                .rotated(Vector3::UP, ang)
                .translated(pivote);
            self.base_mut().set_global_transform(t2);
            self.giro_listo = false;
        } else if sx.abs() < 0.3 {
            self.giro_listo = true;
        }

        self.ajustar_suelo_y_limite(cabeza.get_global_position());
    }

    fn ajustar_suelo_y_limite(&mut self, cam: Vector3) {
        let mut origen = self.base().get_global_position();
        let horiz = Vector2::new(cam.x, cam.z);
        let d = horiz.length();
        if d > self.radio_limite {
            let exceso = horiz.normalized() * (d - self.radio_limite);
            origen.x -= exceso.x;
            origen.z -= exceso.y;
        }
        if let Some(t) = &self.terreno {
            origen.y = t.bind().altura(cam.x, cam.z);
        }
        self.base_mut().set_global_position(origen);
    }

    fn iniciar_escritorio(&mut self) {
        // VRS en modo XR sin casco deja la imagen en negro: se apaga en la PC.
        if let Some(mut vp) = self.base().get_viewport() {
            vp.set_vrs_mode(VrsMode::DISABLED);
        }
        // Sin casco no hay controles: los guantes quedarían flotando.
        for mano in [self.mano_izq.as_mut(), self.mano_der.as_mut()].into_iter().flatten() {
            mano.hide();
        }
        let p = self.base().get_global_position();
        let mut pos = p;
        // --vista=x,alto,z,yaw,pitch (grados)
        if let Some(v) = argumento_usuario("vista") {
            let n: Vec<f32> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
            if n.len() >= 3 {
                pos = Vector3::new(n[0], 0.0, n[2]);
                self.altura_escritorio = n[1];
            }
            if n.len() >= 4 {
                self.yaw = n[3].to_radians();
            }
            if n.len() >= 5 {
                self.pitch = n[4].to_radians();
            }
        } else {
            // conservar la orientación con que se ubicó al jugador en la escena
            let fwd = -self.base().get_global_transform().basis.col_c();
            self.yaw = (-fwd.x).atan2(-fwd.z);
        }
        if let Some(t) = &self.terreno {
            pos.y = t.bind().altura(pos.x, pos.z);
        }
        let b = Basis::from_axis_angle(Vector3::UP, self.yaw);
        self.base_mut().set_global_transform(Transform3D::new(b, pos));
        let alto = self.altura_escritorio;
        let pitch = self.pitch;
        if let Some(c) = self.cabeza.as_mut() {
            c.set_position(Vector3::new(0.0, alto, 0.0));
            c.set_rotation(Vector3::new(pitch, 0.0, 0.0));
            c.set_current(true);
        }
    }

    fn control_escritorio(&mut self, dt: f32) {
        let input = Input::singleton();
        let mut dir = Vector2::ZERO;
        if input.is_key_pressed(Key::W) {
            dir.y += 1.0;
        }
        if input.is_key_pressed(Key::S) {
            dir.y -= 1.0;
        }
        if input.is_key_pressed(Key::A) {
            dir.x -= 1.0;
        }
        if input.is_key_pressed(Key::D) {
            dir.x += 1.0;
        }
        let rapido = if input.is_key_pressed(Key::SHIFT) { 3.0 } else { 1.0 };

        let b = Basis::from_axis_angle(Vector3::UP, self.yaw);
        let mut pos = self.base().get_global_position();
        if dir != Vector2::ZERO {
            let d = dir.normalized();
            let adelante = b * Vector3::FORWARD;
            let derecha = b * Vector3::RIGHT;
            pos += (derecha * d.x + adelante * d.y) * self.velocidad * rapido * dt;
        }
        let horiz = Vector2::new(pos.x, pos.z);
        if horiz.length() > self.radio_limite {
            let h = horiz.normalized() * self.radio_limite;
            pos.x = h.x;
            pos.z = h.y;
        }
        if let Some(t) = &self.terreno {
            pos.y = t.bind().altura(pos.x, pos.z);
        }
        self.base_mut().set_global_transform(Transform3D::new(b, pos));
        let pitch = self.pitch;
        if let Some(c) = self.cabeza.as_mut() {
            c.set_rotation(Vector3::new(pitch, 0.0, 0.0));
        }
    }

    fn actualizar_titulo(&mut self) {
        let (d, t) = (self.duracion_titulo, self.tiempo);
        let Some(titulo) = self.titulo.as_mut() else {
            return;
        };
        if !titulo.is_visible() {
            return;
        }
        let alfa = if t < d { 1.0 } else { (1.0 - (t - d) / 2.5).max(0.0) };
        if (titulo.get_modulate().a - alfa).abs() > 1e-3 {
            // El modulate de Label3D no se hereda: se aplica también a los
            // Label3D hijos (subtítulo).
            let mut etiquetas = vec![titulo.clone()];
            for hijo in titulo.get_children().iter_shared() {
                if let Ok(l) = hijo.try_cast::<Label3D>() {
                    etiquetas.push(l);
                }
            }
            for mut l in etiquetas {
                let mut c = l.get_modulate();
                c.a = alfa;
                l.set_modulate(c);
                let mut o = l.get_outline_modulate();
                o.a = alfa;
                l.set_outline_modulate(o);
            }
        }
        if alfa <= 0.0 {
            titulo.hide();
        }
    }

    fn tomar_captura(&mut self) {
        let Some(ruta) = self.captura.clone() else {
            return;
        };
        self.cuadros += 1;
        if self.cuadros < self.esperar_cuadros {
            return;
        }
        self.captura = None;
        let imagen = self
            .base()
            .get_viewport()
            .and_then(|vp| vp.get_texture())
            .and_then(|tex| tex.get_image());
        match imagen {
            Some(img) => {
                let r = img.save_png(&ruta);
                godot_print!("Captura guardada en {ruta} ({r:?})");
            }
            None => {
                godot_error!("No se pudo leer la imagen del viewport");
            }
        }
        self.base().get_tree().quit();
    }
}
