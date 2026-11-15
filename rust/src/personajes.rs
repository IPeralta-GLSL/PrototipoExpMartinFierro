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

use godot::classes::{Engine, ISkeletonModifier3D, Skeleton3D, SkeletonModifier3D, VisualInstance3D};
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

// ---------------------------------------------------------------------------
// Esquivar obstáculos al caminar (NPCs): cada modelo es una cápsula en planta
// (un segmento con radio, a partir de su caja); también se esquivan los
// objetos sueltos, los otros personajes y el jugador.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Capsula {
    a: Vector2,
    b: Vector2,
    r: f32,
}

fn punto_segmento(p: Vector2, a: Vector2, b: Vector2) -> Vector2 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    a + ab * t
}

/// Huella en planta de un modelo (cápsula a lo largo de su lado más largo).
fn huella(n: &Gd<Node>) -> Option<Capsula> {
    fn juntar(n: &Gd<Node>, caja: &mut Option<Aabb>) {
        if let Ok(v) = n.clone().try_cast::<VisualInstance3D>() {
            if v.is_visible_in_tree() {
                let g = v.get_global_transform() * v.get_aabb();
                *caja = Some(match caja {
                    Some(c) => c.merge(g),
                    None => g,
                });
            }
        }
        for h in n.get_children().iter_shared() {
            juntar(&h, caja);
        }
    }
    let mut caja = None;
    juntar(n, &mut caja);
    let c = caja?;
    let (sx, sz) = (c.size.x, c.size.z);
    if c.size.y < 0.04 || sx.max(sz) < 0.08 || sx.max(sz) > 14.0 {
        return None;
    }
    let centro = c.center();
    let (cx, cz) = (centro.x, centro.z);
    Some(if sx >= sz {
        let medio = (sx - sz) * 0.5;
        Capsula { a: Vector2::new(cx - medio, cz), b: Vector2::new(cx + medio, cz), r: sz * 0.45 }
    } else {
        let medio = (sz - sx) * 0.5;
        Capsula { a: Vector2::new(cx, cz - medio), b: Vector2::new(cx, cz + medio), r: sx * 0.45 }
    })
}

/// Obstáculos fijos de la escena: todos los modelos, salvo los personajes que
/// caminan, los objetos que se agarran, los accesorios y uno mismo.
fn obstaculos_estaticos(yo: &Gd<Node3D>) -> Vec<Capsula> {
    fn recorrer(n: &Gd<Node>, yo: &Gd<Node>, out: &mut Vec<Capsula>) {
        if n == yo
            || n.clone().try_cast::<Caminante>().is_ok()
            || n.clone().try_cast::<crate::narrador::Narrador>().is_ok()
            || n.clone().try_cast::<crate::interaccion::Agarrable>().is_ok()
            || n.clone().try_cast::<Accesorio>().is_ok()
            || n.clone().try_cast::<crate::jugador::JugadorXR>().is_ok()
        {
            return;
        }
        if n.clone().try_cast::<ModeloIA>().is_ok() {
            if let Some(c) = huella(n) {
                out.push(c);
            }
            return;
        }
        for h in n.get_children().iter_shared() {
            recorrer(&h, yo, out);
        }
    }
    let mut out = Vec::new();
    if let Some(escena) = yo.get_tree().get_current_scene() {
        recorrer(&escena, &yo.clone().upcast::<Node>(), &mut out);
    }
    out
}

/// Lo que se mueve: otros caminantes, el narrador, objetos sueltos y el jugador.
fn obstaculos_dinamicos(yo: &Gd<Node3D>) -> Vec<Capsula> {
    let arbol = yo.get_tree();
    let propio = yo.clone().upcast::<Node>();
    let mut out = Vec::new();
    let mut agregar = |p: Vector3, r: f32| {
        let q = Vector2::new(p.x, p.z);
        out.push(Capsula { a: q, b: q, r });
    };
    for grupo in ["caminante", "narrador"] {
        for n in arbol.get_nodes_in_group(grupo).iter_shared() {
            if n == propio || propio.is_ancestor_of(&n) || n.is_ancestor_of(&propio) {
                continue;
            }
            if let Ok(n3) = n.try_cast::<Node3D>() {
                agregar(n3.get_global_position(), 0.3);
            }
        }
    }
    for n in arbol.get_nodes_in_group("agarrable").iter_shared() {
        if let Ok(n3) = n.try_cast::<Node3D>() {
            if n3.is_visible_in_tree() {
                agregar(n3.get_global_position(), 0.15);
            }
        }
    }
    for n in arbol.get_nodes_in_group("jugador").iter_shared() {
        if let Some(cab) = n.get_node_or_null("Cabeza").and_then(|c| c.try_cast::<Node3D>().ok()) {
            agregar(cab.get_global_position(), 0.3);
        }
    }
    out
}

/// Dirección (en planta) hacia el objetivo, apartándose de lo cercano y
/// rodeando por el costado lo que está adelante.
fn dirigir(pos: Vector2, objetivo: Vector2, radio: f32, grupos: &[&[Capsula]]) -> Vector2 {
    let to = objetivo - pos;
    let dist = to.length();
    if dist < 1e-4 {
        return Vector2::ZERO;
    }
    let d = to / dist;
    let mut dir = d;
    for obs in grupos {
        for c in obs.iter() {
            let q = punto_segmento(pos, c.a, c.b);
            let off = pos - q;
            let dd = off.length();
            let limite = c.r + radio + 0.15;
            let zona = limite + 0.9;
            if dd >= zona || dd < 1e-4 {
                continue;
            }
            // Si el objetivo está pegado a este obstáculo, se lo deja acercar.
            let oq = punto_segmento(objetivo, c.a, c.b);
            if (objetivo - oq).length() < c.r + radio {
                continue;
            }
            let n = off / dd;
            if dd < limite {
                dir += n * (1.0 - dd / limite) * 2.5;
            }
            let frente = (-n).dot(d).max(0.0);
            if frente > 0.0 {
                let tang = Vector2::new(-n.y, n.x);
                let s = if tang.dot(d) >= 0.0 { 1.0 } else { -1.0 };
                let f = 1.0 - (dd - limite).max(0.0) / 0.9;
                dir += tang * (s * f * frente * 1.8);
            }
        }
    }
    let l = dir.length();
    if l < 1e-4 {
        d
    } else {
        dir / l
    }
}

/// Lo usan Caminante y Narrador: obstáculos fijos (una vez) + móviles, y
/// detecta si se trabó (no se acerca al objetivo en 4 s).
pub struct Evitador {
    estaticos: Vec<Capsula>,
    listo: bool,
    mejor: f32,
    sin_avance: f32,
}

impl Evitador {
    pub fn nuevo() -> Self {
        Self { estaticos: Vec::new(), listo: false, mejor: f32::MAX, sin_avance: 0.0 }
    }

    pub fn reiniciar(&mut self) {
        self.mejor = f32::MAX;
        self.sin_avance = 0.0;
    }

    /// (dirección en planta, trabado)
    pub fn paso(&mut self, yo: &Gd<Node3D>, objetivo: Vector3, radio: f32, dt: f32) -> (Vector2, bool) {
        if !self.listo {
            self.estaticos = obstaculos_estaticos(yo);
            self.listo = true;
        }
        let pos = yo.get_global_position();
        let p = Vector2::new(pos.x, pos.z);
        let o = Vector2::new(objetivo.x, objetivo.z);
        let dinamicos = obstaculos_dinamicos(yo);
        let dir = dirigir(p, o, radio, &[&self.estaticos, &dinamicos]);
        let dist = (o - p).length();
        if dist < self.mejor - 0.1 {
            self.mejor = dist;
            self.sin_avance = 0.0;
        } else {
            self.sin_avance += dt;
        }
        (dir, self.sin_avance > 4.0)
    }
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
    /// Radio del personaje para esquivar (m).
    #[export]
    radio: f32,

    indice: usize,
    esperando: f32,
    caminando: bool,
    evitador: Evitador,
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
            radio: 0.3,
            indice: 0,
            esperando: 0.5,
            caminando: false,
            evitador: Evitador::nuevo(),
            base,
        }
    }

    fn ready(&mut self) {
        if Engine::singleton().is_editor_hint() {
            self.base_mut().set_process(false);
            return;
        }
        self.base_mut().add_to_group("caminante");
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
        let radio = self.radio;
        let (dir, trabado) = self.evitador.paso(&nodo, objetivo, radio, dt);
        let mut pos = nodo.get_global_position();
        let dist = Vector2::new(objetivo.x - pos.x, objetivo.z - pos.z).length();
        if dist < 0.08 || trabado {
            self.indice = (self.indice + 1) % n;
            self.esperando = self.pausa;
            self.caminando = false;
            self.evitador.reiniciar();
            let a = self.animacion_pausa.clone();
            self.animar(a);
            return;
        }
        // Mira hacia donde camina (esquivando), no al punto de destino.
        let falta = girar_hacia(&mut nodo, pos + Vector3::new(dir.x, 0.0, dir.y), 3.0, dt);
        let v = self.velocidad * if falta > 0.8 { 0.3 } else { 1.0 };
        let paso = (v * dt).min(dist);
        pos.x += dir.x * paso;
        pos.z += dir.y * paso;
        if let Some(h) = altura_global(&self.terreno, pos) {
            pos.y = h;
        }
        nodo.set_global_position(pos);
    }
}

#[godot_api]
impl Caminante {
    /// Se queda quieto unos segundos (sin cambiar la animación: la pone quien lo pausa).
    #[func]
    pub fn pausar(&mut self, segundos: f32) {
        self.esperando = self.esperando.max(segundos);
        self.caminando = false;
    }
}

impl Caminante {
    fn animar(&mut self, candidatos: GString) {
        if let Some(m) = self.modelo.as_mut() {
            m.bind_mut().reproducir(candidatos);
        }
    }
}
