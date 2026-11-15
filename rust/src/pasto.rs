//! Pasto y cortaderas instanciados con MultiMesh. Se reparten en parcelas
//! (una MultiMeshInstance3D por parcela) para que Godot descarte las que
//! quedan fuera de la vista y para cortar por distancia.

use std::collections::HashMap;
use std::f32::consts::TAU;

use godot::classes::{
    geometry_instance_3d::ShadowCastingSetting, multi_mesh::TransformFormat, Material, MultiMesh,
    MultiMeshInstance3D,
};
use godot::prelude::*;

use crate::azar::Azar;
use crate::conversion::a_array_mesh;
use crate::geometria::{malla_cortadera, malla_mata_pasto};
use crate::terreno::TerrenoPampa;

#[derive(GodotClass)]
#[class(tool, base = Node3D)]
pub struct PastoPampa {
    #[export]
    terreno: Option<Gd<TerrenoPampa>>,
    #[export]
    material: Option<Gd<Material>>,
    /// true = cortadera (paja brava con penachos); false = mata de pasto.
    #[export]
    cortadera: bool,
    #[export]
    cantidad: i32,
    #[export]
    radio: f32,
    #[export]
    radio_interno: f32,
    #[export]
    tamano_parcela: f32,
    #[export]
    distancia_visible: f32,
    #[export]
    semilla: i64,
    #[export]
    alto: f32,
    #[export]
    hojas: i32,
    #[export]
    ancho_hoja: f32,
    #[export]
    escala_min: f32,
    #[export]
    escala_max: f32,
    #[export]
    color_a: Color,
    #[export]
    color_b: Color,
    #[export]
    sombras: bool,

    parcelas: Vec<Gd<MultiMeshInstance3D>>,
    base: Base<Node3D>,
}

#[godot_api]
impl INode3D for PastoPampa {
    fn init(base: Base<Node3D>) -> Self {
        Self {
            terreno: None,
            material: None,
            cortadera: false,
            cantidad: 6000,
            radio: 40.0,
            radio_interno: 0.0,
            tamano_parcela: 12.0,
            distancia_visible: 60.0,
            semilla: 7,
            alto: 0.42,
            hojas: 5,
            ancho_hoja: 0.018,
            escala_min: 0.7,
            escala_max: 1.3,
            color_a: Color::from_rgb(0.55, 0.62, 0.30),
            color_b: Color::from_rgb(0.84, 0.74, 0.43),
            sombras: false,
            parcelas: Vec::new(),
            base,
        }
    }

    fn ready(&mut self) {
        self.regenerar();
    }
}

#[inline]
fn suave(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[godot_api]
impl PastoPampa {
    /// Vuelve a sembrar todo (también desde el editor).
    #[func]
    pub fn regenerar(&mut self) {
        for mut p in self.parcelas.drain(..) {
            p.queue_free();
        }
        let Some(terreno) = self.terreno.clone() else {
            godot_warn!("PastoPampa: falta asignar 'terreno'.");
            return;
        };

        let semilla = self.semilla as u64;
        let datos = if self.cortadera {
            malla_cortadera(semilla)
        } else {
            malla_mata_pasto(semilla, self.hojas.max(1) as u32, self.alto, self.ancho_hoja)
        };
        let malla = a_array_mesh(&datos);

        // Siembra por muestreo de rechazo: menos densidad lejos y nada
        // sobre la tierra pisada.
        let mut az = Azar::new(semilla ^ 0xA5A5_5A5A);
        let tam = self.tamano_parcela.max(2.0);
        let mut por_parcela: HashMap<(i32, i32), Vec<(Transform3D, Color)>> = HashMap::new();
        {
            let t = terreno.bind();
            let objetivo = self.cantidad.max(0) as usize;
            let max_intentos = objetivo * 10 + 100;
            let (mut colocadas, mut intentos) = (0usize, 0usize);
            while colocadas < objetivo && intentos < max_intentos {
                intentos += 1;
                let r = self.radio * az.f32().sqrt();
                if r < self.radio_interno {
                    continue;
                }
                let ang = az.rango(0.0, TAU);
                let (x, z) = (r * ang.cos(), r * ang.sin());
                let caida = 1.0 - 0.75 * suave(0.55 * self.radio, self.radio, r);
                let cob = t.cobertura(x, z);
                if az.f32() > cob * cob * caida {
                    continue;
                }
                let y = t.altura(x, z);
                let s = az.rango(self.escala_min, self.escala_max.max(self.escala_min));
                let mut basis = Basis::from_axis_angle(Vector3::UP, az.rango(0.0, TAU));
                if self.cortadera {
                    // leve inclinación natural de las matas grandes
                    basis = Basis::from_axis_angle(Vector3::RIGHT, az.rango(-0.08, 0.08)) * basis;
                }
                basis = basis * Basis::from_scale(Vector3::splat(s));
                let tinte = self.color_a.lerp(self.color_b, az.f32() as f64);
                let brillo = az.rango(0.88, 1.08);
                let color = Color::from_rgba(
                    (tinte.r * brillo).min(1.0),
                    (tinte.g * brillo).min(1.0),
                    (tinte.b * brillo).min(1.0),
                    1.0,
                );
                let clave = ((x / tam).floor() as i32, (z / tam).floor() as i32);
                por_parcela
                    .entry(clave)
                    .or_default()
                    .push((Transform3D::new(basis, Vector3::new(x, y, z)), color));
                colocadas += 1;
            }
            if colocadas < objetivo {
                godot_print!(
                    "PastoPampa: sembradas {colocadas} de {objetivo} (mucha tierra pisada o radio chico)"
                );
            }
        }

        let sombra = if self.sombras {
            ShadowCastingSetting::ON
        } else {
            ShadowCastingSetting::OFF
        };
        let mut claves: Vec<_> = por_parcela.keys().copied().collect();
        claves.sort();
        for clave in claves {
            let lista = &por_parcela[&clave];
            let mut mm = MultiMesh::new_gd();
            mm.set_transform_format(TransformFormat::TRANSFORM_3D);
            mm.set_use_colors(true);
            mm.set_mesh(&malla);
            mm.set_instance_count(lista.len() as i32);
            for (i, (xf, c)) in lista.iter().enumerate() {
                mm.set_instance_transform(i as i32, *xf);
                mm.set_instance_color(i as i32, *c);
            }
            let mut mmi = MultiMeshInstance3D::new_alloc();
            mmi.set_name(&format!("Parcela_{}_{}", clave.0, clave.1));
            mmi.set_multimesh(&mm);
            if let Some(mat) = &self.material {
                mmi.set_material_override(mat);
            }
            mmi.set_cast_shadows_setting(sombra);
            mmi.set_visibility_range_end(self.distancia_visible);
            // el viento mueve las puntas fuera del AABB original
            mmi.set_extra_cull_margin(0.6);
            self.base_mut().add_child(&mmi);
            self.parcelas.push(mmi);
        }
    }
}
