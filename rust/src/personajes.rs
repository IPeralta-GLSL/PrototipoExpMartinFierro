//! Personajes animados (riggeados con tools/rig): objetos en la mano y
//! recorridos a pie.
//!
//! - `Accesorio`: sigue un hueso del esqueleto de un `ModeloIA` (el mate, el
//!   fusil, la pala). Sus hijos son el objeto; su transformación local es el
//!   ajuste respecto del hueso. Puede mostrarse solo durante ciertas
//!   animaciones ("mate").
//! - `Caminante`: lleva a un personaje por una lista de puntos, con la
//!   animación de caminar y pausas (la ronda del sargento en el fortín).

use std::f32::consts::{PI, TAU};

use godot::classes::{Engine, ISkeletonModifier3D, Skeleton3D, SkeletonModifier3D};
use godot::prelude::*;

use crate::modelos::ModeloIA;
use crate::terreno::TerrenoPampa;

/// Altura global del terreno debajo de `p` (si hay terreno).
pub fn altura_global(terreno: &Option<Gd<TerrenoPampa>>, p: Vector3) -> Option<f32> {
    let t = terreno.as_ref()?;
    let xf = t.get_global_transform();
    let local = xf.affine_inverse() * p;
    let h = t.bind().altura(local.x, local.z);
    Some((xf * Vector3::new(local.x, h, local.z)).y)
}

/// Gira el nodo en Y hacia `objetivo` (global) a `velocidad` rad/s.
/// Los modelos miran hacia +Z. Devuelve el ángulo que falta.
pub fn girar_hacia(nodo: &mut Gd<Node3D>, objetivo: Vector3, velocidad: f32, dt: f32) -> f32 {
    let p = nodo.get_global_position();
    let (dx, dz) = (objetivo.x - p.x, objetivo.z - p.z);
    if dx * dx + dz * dz < 1e-6 {
        return 0.0;
    }
    let deseado = dx.atan2(dz);
    let mut r = nodo.get_global_rotation();
    let mut dif = deseado - r.y;
    while dif > PI {
        dif -= TAU;
    }
    while dif < -PI {
        dif += TAU;
    }
    let paso = velocidad * dt;
    r.y += dif.clamp(-paso, paso);
    r.x = 0.0;
    r.z = 0.0;
    nodo.set_global_rotation(r);
    dif.abs()
}

fn coincide(actual: &str, candidatos: &str) -> bool {
    let a = actual.to_lowercase();
    candidatos
        .split('|')
        .map(|c| c.trim().to_lowercase())
        .any(|c| !c.is_empty() && a.contains(&c))
}

// ---------------------------------------------------------------------------

#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct Accesorio {
    /// Personaje que lo lleva.
    #[export]
    portador: Option<Gd<ModeloIA>>,
    /// Hueso del que cuelga ("mano.R", "mano.L", "cabeza"...).
    #[export]
    hueso: GString,
    /// Visible solo con estas animaciones ("mate|..."); vacío = siempre.
    #[export]
    solo_durante: GString,
    /// Sigue la palma pero se mantiene derecho, girado como el portador
    /// (fusil al costado, mate). El ajuste es relativo a ese marco.
    #[export]
    vertical: bool,
    /// Si se indica (p. ej. "mano.L"), el eje +X del objeto va de ese hueso
    /// hacia `hueso` (una pala tomada con las dos manos).
    #[export]
    hacia_hueso: GString,
    /// Punto de agarre en el espacio del hueso (Y = a lo largo de la mano,
    /// Z = hacia la palma): ahí queda el objeto, dentro de los dedos cerrados.
    #[export]
    agarre: Vector3,

    esqueleto: Option<Gd<Skeleton3D>>,
    indice: i32,
    indice2: i32,
    ajuste: Transform3D,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for Accesorio {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            portador: None,
            hueso: GString::from("mano.R"),
            solo_durante: GString::new(),
            vertical: false,
            hacia_hueso: GString::new(),
            agarre: Vector3::new(0.0, 0.05, 0.035),
            esqueleto: None,
            indice: -1,
            indice2: -1,
            ajuste: Transform3D::IDENTITY,
            base,
        }
    }

    fn ready(&mut self) {
        if Engine::singleton().is_editor_hint() {
            self.base_mut().set_process(false);
            return;
        }
        // La transformación puesta en la escena es el ajuste respecto del hueso.
        self.ajuste = self.base().get_transform();
        // Después de que la animación movió el esqueleto.
        self.base_mut().set_process_priority(100);
    }

    fn process(&mut self, _delta: f64) {
        let Some(portador) = self.portador.clone() else {
            return;
        };
        if self.esqueleto.is_none() {
            self.esqueleto = portador.bind().esqueleto();
            if let Some(s) = &self.esqueleto {
                self.indice = s.find_bone(&self.hueso.to_string());
                if self.indice < 0 {
                    godot_warn!("Accesorio: el esqueleto no tiene el hueso '{}'", self.hueso);
                }
                if !self.hacia_hueso.is_empty() {
                    self.indice2 = s.find_bone(&self.hacia_hueso.to_string());
                }
            }
        }
        let Some(sk) = self.esqueleto.clone() else {
            self.base_mut().hide();
            return;
        };
        if self.indice < 0 {
            return;
        }
        let visible = self.solo_durante.is_empty()
            || coincide(&portador.bind().animacion_actual().to_string(), &self.solo_durante.to_string());
        self.base_mut().set_visible(visible);
        if !visible {
            return;
        }
        let gsk = sk.get_global_transform();
        let tb = gsk * sk.get_bone_global_pose(self.indice);
        let palma = tb * self.agarre;
        let pose = if self.indice2 >= 0 {
            // De una mano a la otra (la pala): +X del objeto sobre esa línea.
            let t2 = gsk * sk.get_bone_global_pose(self.indice2);
            let otra = t2 * self.agarre;
            let x = (palma - otra).normalized();
            let mut z = x.cross(Vector3::UP);
            if z.length() < 1e-3 {
                z = Vector3::BACK;
            }
            let z = z.normalized();
            let y = z.cross(x).normalized();
            Transform3D::new(Basis::from_cols(x, y, z), palma) * self.ajuste
        } else if self.vertical {
            let yaw = portador.get_global_rotation().y;
            let b = Basis::from_euler(EulerOrder::YXZ, Vector3::new(0.0, yaw, 0.0));
            Transform3D::new(b, palma) * self.ajuste
        } else {
            tb * self.ajuste
        };
        self.base_mut().set_global_transform(pose);
    }
}

// ---------------------------------------------------------------------------

/// Abre la mandíbula (hueso "mandibula" de tools/rig) después de la animación.
/// El narrador le pasa la `apertura` (0..1) según el volumen de la voz.
#[derive(GodotClass)]
#[class(base = SkeletonModifier3D)]
pub struct Mandibula {
    /// 0 = cerrada, 1 = abierta del todo.
    #[var]
    pub apertura: f32,
    /// Giro máximo en grados sobre el X local del hueso (negativo = abre).
    #[export]
    grados: f32,
    #[export]
    hueso: GString,
    indice: i32,
    base: Base<SkeletonModifier3D>,
}

#[godot_api]
impl ISkeletonModifier3D for Mandibula {
    fn init(base: Base<SkeletonModifier3D>) -> Self {
        Self { apertura: 0.0, grados: -12.0, hueso: GString::from("mandibula"), indice: -1, base }
    }

    fn process_modification_with_delta(&mut self, _delta: f64) {
        let Some(mut sk) = self.base().get_skeleton() else {
            return;
        };
        if self.indice < 0 {
            self.indice = sk.find_bone(&self.hueso.to_string());
            if self.indice < 0 {
                return;
            }
        }
        let reposo = sk.get_bone_rest(self.indice).basis.get_quaternion();
        let giro = Quaternion::from_axis_angle(Vector3::RIGHT, (self.grados * self.apertura).to_radians());
        sk.set_bone_pose_rotation(self.indice, reposo * giro);
    }
}

// ---------------------------------------------------------------------------

#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct Caminante {
    /// El personaje (hijo de este nodo); se le cambia la animación.
    #[export]
    modelo: Option<Gd<ModeloIA>>,
    /// Recorrido, en el espacio del padre. Se repite en bucle.
    #[export]
    puntos: PackedVector3Array,
    /// Metros por segundo (con el ciclo de caminar de tools/rig, ~1.0).
    #[export]
    velocidad: f32,
    /// Segundos de pausa en cada punto.
    #[export]
    pausa: f32,
    #[export]
    animacion_caminar: GString,
    #[export]
    animacion_pausa: GString,
    #[export]
    terreno: Option<Gd<TerrenoPampa>>,

    indice: usize,
    esperando: f32,
    caminando: bool,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for Caminante {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            modelo: None,
            puntos: PackedVector3Array::new(),
            velocidad: 1.0,
            pausa: 3.0,
            animacion_caminar: GString::from("caminar|walk"),
            animacion_pausa: GString::from("idle"),
            terreno: None,
            indice: 0,
            esperando: 0.5,
            caminando: false,
            base,
        }
    }

    fn ready(&mut self) {
        if Engine::singleton().is_editor_hint() {
            self.base_mut().set_process(false);
            return;
        }
        let a = self.animacion_pausa.clone();
        self.animar(a);
    }

    fn process(&mut self, delta: f64) {
        let n = self.puntos.len();
        if n == 0 {
            return;
        }
        let dt = delta as f32;
        if self.esperando > 0.0 {
            self.esperando -= dt;
            return;
        }
        if !self.caminando {
            self.caminando = true;
            let a = self.animacion_caminar.clone();
            self.animar(a);
        }
        let local = self.puntos.as_slice()[self.indice % n];
        let objetivo = match self.base().get_parent().and_then(|p| p.try_cast::<Node3D>().ok()) {
            Some(p) => p.get_global_transform() * local,
            None => local,
        };
        let mut nodo = self.base().clone().upcast::<Node3D>();
        let falta = girar_hacia(&mut nodo, objetivo, 3.0, dt);
        let mut pos = nodo.get_global_position();
        let d = Vector2::new(objetivo.x - pos.x, objetivo.z - pos.z);
        let dist = d.length();
        if dist < 0.06 {
            self.indice = (self.indice + 1) % n;
            self.esperando = self.pausa;
            self.caminando = false;
            let a = self.animacion_pausa.clone();
            self.animar(a);
            return;
        }
        // Si todavía está girando mucho, avanza más despacio.
        let v = self.velocidad * if falta > 0.8 { 0.3 } else { 1.0 };
        let paso = (v * dt).min(dist);
        pos.x += d.x / dist * paso;
        pos.z += d.y / dist * paso;
        if let Some(h) = altura_global(&self.terreno, pos) {
            pos.y = h;
        }
        nodo.set_global_position(pos);
    }
}

impl Caminante {
    fn animar(&mut self, candidatos: GString) {
        if let Some(m) = self.modelo.as_mut() {
            m.bind_mut().reproducir(candidatos);
        }
    }
}
