//! Vida en el paisaje: pájaros que vuelan y cosas que giran con el viento.
//!
//! - `Bandada`: pájaros procedurales (cuerpo y dos alas que aletean). Vuelan en
//!   círculos irregulares alrededor de un centro; los chicos aletean rápido en
//!   bandada, los grandes (chimangos) planean con aletazos de vez en cuando.
//!   Con `irse_tras` > 0 se van (suben y se alejan) a los N segundos: de noche
//!   no quedan pájaros en el cielo.
//! - `Giratorio`: gira sobre su eje Z con rachas (la rueda del molino).

use std::f32::consts::TAU;

use godot::classes::{base_material_3d::ShadingMode, BoxMesh, Engine, MeshInstance3D, StandardMaterial3D};
use godot::prelude::*;

use crate::azar::Azar;
use crate::ruido::Perlin2;

struct Pajaro {
    nodo: Gd<Node3D>,
    ala_izq: Gd<Node3D>,
    ala_der: Gd<Node3D>,
    radio: f32,
    alto: f32,
    fase: f32,
    vel: f32,
    aleteo: f32,
    semilla: f32,
}

#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct Bandada {
    #[export]
    cantidad: i32,
    /// Envergadura (m): ~0.35 pajaritos, ~1.1 chimangos.
    #[export]
    envergadura: f32,
    /// Radio medio del círculo de vuelo (m).
    #[export]
    radio: f32,
    /// Altura media de vuelo (m sobre este nodo).
    #[export]
    altura: f32,
    /// Velocidad (m/s).
    #[export]
    velocidad: f32,
    /// true = planean (chimangos); false = aletean en bandada (pajaritos).
    #[export]
    planeadores: bool,
    /// Segundos hasta que se van (0 = nunca).
    #[export]
    irse_tras: f32,
    #[export]
    semilla: i64,

    pajaros: Vec<Pajaro>,
    t: f32,
    ruido: Perlin2,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for Bandada {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            cantidad: 9,
            envergadura: 0.35,
            radio: 18.0,
            altura: 14.0,
            velocidad: 7.0,
            planeadores: false,
            irse_tras: 0.0,
            semilla: 7,
            pajaros: Vec::new(),
            t: 0.0,
            ruido: Perlin2::new(7),
            base,
        }
    }

    fn ready(&mut self) {
        if Engine::singleton().is_editor_hint() {
            return;
        }
        self.ruido = Perlin2::new(self.semilla as u64);
        let mut az = Azar::new(self.semilla as u64);
        let mut mat = StandardMaterial3D::new_gd();
        mat.set_albedo(Color::from_rgb(0.09, 0.08, 0.07));
        mat.set_shading_mode(ShadingMode::UNSHADED);
        let e = self.envergadura;
        let caja = |t: Vector3| {
            let mut m = BoxMesh::new_gd();
            m.set_size(t);
            m.set_material(&mat);
            m
        };
        let cuerpo_m = caja(Vector3::new(e * 0.14, e * 0.12, e * 0.6));
        let ala_m = caja(Vector3::new(e * 0.5, e * 0.02, e * 0.32));
        let cola_m = caja(Vector3::new(e * 0.18, e * 0.015, e * 0.22));
        for i in 0..self.cantidad.max(0) {
            let mut nodo = Node3D::new_alloc();
            nodo.set_name(&format!("Pajaro{i}"));
            let mut cuerpo = MeshInstance3D::new_alloc();
            cuerpo.set_mesh(&cuerpo_m);
            nodo.add_child(&cuerpo);
            let mut cola = MeshInstance3D::new_alloc();
            cola.set_mesh(&cola_m);
            cola.set_position(Vector3::new(0.0, 0.0, e * 0.38));
            nodo.add_child(&cola);
            let mut alas = Vec::new();
            for lado in [-1.0f32, 1.0] {
                // El ala gira sobre una bisagra en el costado del cuerpo.
                let mut bisagra = Node3D::new_alloc();
                bisagra.set_position(Vector3::new(lado * e * 0.07, 0.0, -e * 0.03));
                let mut ala = MeshInstance3D::new_alloc();
                ala.set_mesh(&ala_m);
                ala.set_position(Vector3::new(lado * e * 0.25, 0.0, 0.0));
                bisagra.add_child(&ala);
                nodo.add_child(&bisagra);
                alas.push(bisagra);
            }
            self.base_mut().add_child(&nodo);
            let ala_der = alas.pop().unwrap();
            let ala_izq = alas.pop().unwrap();
            self.pajaros.push(Pajaro {
                nodo,
                ala_izq,
                ala_der,
                radio: self.radio * az.rango(0.75, 1.25),
                alto: self.altura * az.rango(0.85, 1.2),
                fase: if self.planeadores { az.rango(0.0, TAU) } else { az.rango(-0.35, 0.35) },
                vel: self.velocidad * az.rango(0.9, 1.1),
                aleteo: az.rango(0.0, TAU),
                semilla: az.rango(0.0, 100.0),
            });
        }
    }

    fn process(&mut self, delta: f64) {
        let dt = delta as f32;
        self.t += dt;
        let t = self.t;
        // Al irse: suben y se alejan; después se ocultan.
        let ida = if self.irse_tras > 0.0 { ((t - self.irse_tras) / 25.0).clamp(0.0, 1.0) } else { 0.0 };
        if ida >= 1.0 {
            self.base_mut().hide();
            return;
        }
        let planea = self.planeadores;
        let e = self.envergadura;
        for i in 0..self.pajaros.len() {
            let (r, alto, fase, vel, sem) = {
                let p = &self.pajaros[i];
                (p.radio, p.alto, p.fase, p.vel, p.semilla)
            };
            let n = &self.ruido;
            // Bandada: todos juntos con un poco de desorden; planeadores: cada uno su círculo.
            let ang = fase + t * vel / r.max(1.0);
            let radio = r * (1.0 + 0.25 * n.ruido(t * 0.07, sem)) * (1.0 + 2.5 * ida);
            let pos = |a: f32| {
                Vector3::new(
                    a.cos() * radio + n.ruido(t * 0.3, sem + 3.0) * 2.0,
                    alto + 2.5 * n.ruido(t * 0.2, sem + 7.0) + ida * 30.0,
                    a.sin() * radio * 0.75 + n.ruido(t * 0.3, sem + 11.0) * 2.0,
                )
            };
            let p0 = pos(ang);
            let p1 = pos(ang + 0.05);
            let p = &mut self.pajaros[i];
            p.nodo.set_position(p0);
            let dir = p1 - p0;
            if dir.length() > 1e-4 {
                let gp = p.nodo.get_global_position();
                p.nodo.look_at_ex(gp + dir).up(Vector3::UP).done();
                // Se inclinan en la curva.
                p.nodo.rotate_object_local(Vector3::BACK, -0.35);
            }
            // Aleteo: rápido y constante (pajaritos) o planeo con aletazos (chimangos).
            let batido = if planea {
                let ciclo = (t * 0.25 + sem).sin();
                if ciclo > 0.7 { (t * 7.0 + p.aleteo).sin() * 0.55 } else { 0.12 }
            } else {
                let pausa = ((t * 0.6 + sem).sin() > 0.85) as i32 as f32;
                (t * 16.0 + p.aleteo).sin() * 0.7 * (1.0 - pausa) + 0.1 * pausa
            };
            p.ala_izq.set_rotation(Vector3::new(0.0, 0.0, -batido));
            p.ala_der.set_rotation(Vector3::new(0.0, 0.0, batido));
            let _ = e;
        }
    }
}

// ---------------------------------------------------------------------------

#[derive(GodotClass)]
#[class(base = Node3D)]
pub struct Giratorio {
    /// Vueltas por segundo (en promedio).
    #[export]
    velocidad: f32,
    t: f32,
    ruido: Perlin2,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for Giratorio {
    fn init(base: Base<Node3D>) -> Self {
        Self { velocidad: 0.35, t: 0.0, ruido: Perlin2::new(1880), base }
    }

    fn process(&mut self, delta: f64) {
        let dt = delta as f32;
        self.t += dt;
        let racha = 1.0 + 0.45 * self.ruido.ruido(self.t * 0.15, 3.3);
        let paso = self.velocidad * racha * TAU * dt;
        self.base_mut().rotate_object_local(Vector3::BACK, paso);
    }
}
