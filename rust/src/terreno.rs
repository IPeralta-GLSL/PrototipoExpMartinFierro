//! Suelo de la pampa: relieve suave, color de pasto/tierra y consultas de altura.

use std::cell::OnceCell;

use godot::classes::{
    geometry_instance_3d::ShadowCastingSetting, Engine, Material, MeshInstance3D,
};
use godot::prelude::*;

use crate::conversion::a_array_mesh;
use crate::geometria::{malla_terreno, Relieve};

/// Nodo del suelo. Genera la malla en un hijo interno (sin dueño), así la
/// geometría procedural no se guarda dentro de la escena al editarla.
#[derive(GodotClass)]
#[class(tool, base = Node3D)]
pub struct TerrenoPampa {
    #[export]
    semilla: i64,
    /// Ondulación máxima del terreno, en metros.
    #[export]
    amplitud: f32,
    /// Tamaño de las lomas, en metros.
    #[export]
    escala: f32,
    /// Radio del campamento casi plano alrededor del origen.
    #[export]
    radio_llano: f32,
    /// Hasta dónde llega el suelo (el horizonte lo tapa la niebla).
    #[export]
    radio_max: f32,
    #[export]
    paso_inicial: f32,
    #[export]
    crecimiento: f32,
    #[export]
    segmentos: i32,
    /// Manchas de tierra pisada: (x, z, radio).
    #[export]
    manchas: PackedVector3Array,
    /// Senderos: pares consecutivos de puntos (x, z, ancho).
    #[export]
    senderos: PackedVector3Array,
    #[export]
    material: Option<Gd<Material>>,

    relieve: OnceCell<Relieve>,
    malla: Option<Gd<MeshInstance3D>>,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for TerrenoPampa {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            semilla: 1872,
            amplitud: 1.3,
            escala: 85.0,
            radio_llano: 14.0,
            radio_max: 1500.0,
            paso_inicial: 0.6,
            crecimiento: 1.045,
            segmentos: 160,
            manchas: PackedVector3Array::new(),
            senderos: PackedVector3Array::new(),
            material: None,
            relieve: OnceCell::new(),
            malla: None,
            base,
        }
    }

    fn ready(&mut self) {
        self.regenerar();
        // Los objetos del grupo "apoyar_en_suelo" se asientan sobre el relieve
        // (diferido: cuando todos los hermanos ya están listos). En el editor
        // no se tocan, para no modificar la escena guardada.
        if !Engine::singleton().is_editor_hint() {
            self.run_deferred(|t| t.apoyar_objetos());
        }
    }
}

#[godot_api]
impl TerrenoPampa {
    /// Reconstruye el relieve y la malla (también desde el editor).
    #[func]
    pub fn regenerar(&mut self) {
        self.relieve = OnceCell::new();
        let datos = malla_terreno(
            self.relieve(),
            self.radio_max,
            self.paso_inicial,
            self.crecimiento,
            self.segmentos.max(8) as u32,
        );
        let malla = a_array_mesh(&datos);

        if let Some(mut vieja) = self.malla.take() {
            vieja.queue_free();
        }
        let mut mi = MeshInstance3D::new_alloc();
        mi.set_name("MallaGenerada");
        mi.set_mesh(&malla);
        if let Some(mat) = &self.material {
            mi.set_material_override(mat);
        }
        // El suelo recibe sombras pero no proyecta (ahorra en el Quest).
        mi.set_cast_shadows_setting(ShadowCastingSetting::OFF);
        self.base_mut().add_child(&mi);
        self.malla = Some(mi);
    }

    /// Altura del suelo (coordenadas locales del terreno = globales).
    #[func]
    pub fn altura(&self, x: f32, z: f32) -> f32 {
        self.relieve().altura(x, z)
    }

    /// 1 = pasto, 0 = tierra pisada.
    #[func]
    pub fn cobertura(&self, x: f32, z: f32) -> f32 {
        self.relieve().cobertura_pasto(x, z)
    }

    #[func]
    pub fn normal_en(&self, x: f32, z: f32) -> Vector3 {
        let n = self.relieve().normal(x, z);
        Vector3::new(n[0], n[1], n[2])
    }

    /// Apoya en el suelo los nodos del grupo "apoyar_en_suelo".
    #[func]
    pub fn apoyar_objetos(&mut self) {
        if !self.base().is_inside_tree() {
            return;
        }
        let arbol = self.base().get_tree();
        let nodos = arbol.get_nodes_in_group("apoyar_en_suelo");
        for nodo in nodos.iter_shared() {
            if let Ok(mut n3) = nodo.try_cast::<Node3D>() {
                let p = n3.get_global_position();
                let y = self.altura(p.x, p.z);
                n3.set_global_position(Vector3::new(p.x, y, p.z));
            }
        }
    }
}

impl TerrenoPampa {
    pub fn relieve(&self) -> &Relieve {
        self.relieve.get_or_init(|| {
            let mut r = Relieve::new(
                self.semilla as u64,
                self.amplitud,
                self.escala,
                self.radio_llano,
            );
            for m in self.manchas.as_slice() {
                r.manchas.push((m.x, m.y, m.z.max(0.1)));
            }
            let s = self.senderos.as_slice();
            for par in s.chunks_exact(2) {
                r.senderos
                    .push(((par[0].x, par[0].y), (par[1].x, par[1].y), par[0].z.max(0.2)));
            }
            r
        })
    }
}
