//! Cel shading pictórico para toda la escena: los StandardMaterial3D opacos pasan
//! al shader `pintura` (luz en dos bandas con el borde a pincel y pinceladas en
//! el color, ver `shaders/pintura.gdshaderinc`). Se apaga con `-- --sin-cel`.
//!
//! Cada material se convierte una sola vez: el resultado queda como metadato del
//! original y lo comparten todas las instancias que lo usan.

use std::sync::OnceLock;

use godot::classes::{
    base_material_3d::{BillboardMode, CullMode, Feature, Flags, ShadingMode, TextureParam, Transparency},
    ArrayMesh, BaseMaterial3D, Engine, GeometryInstance3D, Label3D, Material, MeshInstance3D, Node,
    PrimitiveMesh, Shader, ShaderMaterial, Texture2D,
};
use godot::prelude::*;

use crate::conversion::argumento_usuario;

/// true salvo en el editor o con `-- --sin-cel`.
pub fn activa() -> bool {
    static ACTIVA: OnceLock<bool> = OnceLock::new();
    *ACTIVA.get_or_init(|| !Engine::singleton().is_editor_hint() && argumento_usuario("sin-cel").is_none())
}

/// El material equivalente con el shader de pintura, o None si no corresponde
/// (transparentes, sin sombreado, emisivos, billboards o que ya son shaders).
fn convertir(mat: &Gd<Material>) -> Option<Gd<Material>> {
    let Ok(b) = mat.clone().try_cast::<BaseMaterial3D>() else { return None };
    let clave = StringName::from("pintura");
    if b.has_meta(&clave) {
        return b.get_meta(&clave).try_to::<Gd<Material>>().ok();
    }
    if b.get_transparency() != Transparency::DISABLED
        || b.get_shading_mode() == ShadingMode::UNSHADED
        || b.get_feature(Feature::EMISSION)
        || b.get_billboard_mode() != BillboardMode::DISABLED
    {
        return None;
    }
    let ruta = if b.get_cull_mode() == CullMode::DISABLED {
        "res://shaders/pintura_doble.gdshader"
    } else {
        "res://shaders/pintura.gdshader"
    };
    let shader = try_load::<Shader>(ruta).ok()?;
    let mut m = ShaderMaterial::new_gd();
    m.set_shader(&shader);
    m.set_shader_parameter("color", &b.get_albedo().to_variant());
    if let Some(t) = b.get_texture(TextureParam::ALBEDO) {
        m.set_shader_parameter("textura", &t.to_variant());
    }
    m.set_shader_parameter("usar_color_vertice", &b.get_flag(Flags::ALBEDO_FROM_VERTEX_COLOR).to_variant());
    m.set_shader_parameter("uv1_escala", &b.get_uv1_scale().to_variant());
    m.set_shader_parameter("uv1_desplazamiento", &b.get_uv1_offset().to_variant());
    if let Ok(p) = try_load::<Texture2D>("res://materiales/pincel.tres") {
        m.set_shader_parameter("pincel", &p.to_variant());
    }
    // Ajustes por material (metadatos del original), p. ej. pinceladas más finas
    // en las manos del jugador.
    for param in ["escala_pincel", "fuerza_pincel", "borde_pincel", "tintas"] {
        if b.has_meta(param) {
            m.set_shader_parameter(param, &b.get_meta(param));
        }
    }
    // La línea de tinta sigue como segunda pasada.
    if let Some(sig) = b.get_next_pass() {
        m.set_next_pass(&sig);
    }
    let m = m.upcast::<Material>();
    let mut original = b.clone();
    original.set_meta(&clave, &m.to_variant());
    Some(m)
}

/// Convierte los materiales de `n` y de todo lo que cuelga de él.
pub fn pintar(n: &Gd<Node>) {
    if !activa() || n.clone().try_cast::<Label3D>().is_ok() {
        return;
    }
    if let Ok(mut g) = n.clone().try_cast::<GeometryInstance3D>() {
        if let Some(c) = g.get_material_override().and_then(|m| convertir(&m)) {
            g.set_material_override(&c);
        }
    }
    if let Ok(mut mi) = n.clone().try_cast::<MeshInstance3D>() {
        if let Some(malla) = mi.get_mesh() {
            for i in 0..malla.get_surface_count() {
                if let Some(c) = mi.get_surface_override_material(i).and_then(|m| convertir(&m)) {
                    mi.set_surface_override_material(i, &c);
                }
                let Some(c) = malla.surface_get_material(i).and_then(|m| convertir(&m)) else { continue };
                // En el recurso de la malla (compartido): sirve para todas sus copias.
                if let Ok(mut am) = malla.clone().try_cast::<ArrayMesh>() {
                    am.surface_set_material(i, &c);
                } else if let Ok(mut pm) = malla.clone().try_cast::<PrimitiveMesh>() {
                    pm.set_material(&c);
                } else {
                    mi.set_surface_override_material(i, &c);
                }
            }
        }
    }
    for h in n.get_children().iter_shared() {
        pintar(&h);
    }
}
