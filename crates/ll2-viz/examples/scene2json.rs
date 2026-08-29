//! Dumps a scene as compact JSON, for a renderer that is not this repository.
//!
//! Deliberately the same `Scene` the canvas and the SVG writer consume: the point
//! of it being renderer-agnostic is that a third renderer costs a serialiser.
//!
//! Two things about the numbers. `coords` runs `x, y, z, x, y, z, …` in centimetres,
//! three per vertex, because a Lanelet2 map has an elevation and a file that dropped
//! it would decide for its reader that no renderer downstream is allowed to be
//! three-dimensional; a flat renderer reads every third number and ignores it.
//! `bounds` is `[minX, minY, minZ, maxX, maxY, maxZ]` in the same units, relative to
//! `centre` in x and y and absolute in z.
use std::fmt::Write as _;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: scene2json <map.osm> <out.json> [name]");
    let out_path = args.next().expect("missing output path");
    let name = args.next().unwrap_or_else(|| "map".into());

    let text = std::fs::read_to_string(&path).unwrap();
    let loaded = ll2_viz::load_osm_str(&text, &ll2_viz::LoadOptions::default()).unwrap();
    let options = ll2_viz::VizOptions {
        centerlines: true,
        ..ll2_viz::VizOptions::default()
    };
    let scene = ll2_viz::Scene::from_map(&loaded.map, &options);
    // The same scene under the other palette. Style *names* come from the map's
    // tags, so both tables intern in the same order and one set of shape indices
    // addresses either — which is what lets the page follow the reader's theme.
    let light = ll2_viz::Scene::from_map(
        &loaded.map,
        &ll2_viz::VizOptions {
            theme: ll2_viz::Theme::Light,
            ..options.clone()
        },
    );
    assert_eq!(
        scene.styles.len(),
        light.styles.len(),
        "the two palettes disagree"
    );
    let b = scene.safe_bounds();
    let centre = [(b.min[0] + b.max[0]) * 0.5, (b.min[1] + b.max[1]) * 0.5];

    // Centimetres as integers: a map viewer cannot see finer, and it halves the
    // size of the document against printed floats.
    let cm = |v: f64| (v * 100.0).round() as i64;

    // Three per vertex, not two. A renderer that only wants the map view reads every
    // third number and drops it; one that wants the relief has it, which it would
    // not if this file had done the flattening on its behalf.
    let mut coords = String::from("[");
    let mut offsets = String::from("[");
    let mut style_of = String::from("[");
    let mut layer_of = String::from("[");
    let mut closed = String::from("[");
    let mut labels = String::from("[");
    let mut vertices = 0usize;
    for (index, shape) in scene.shapes.iter().enumerate() {
        if index > 0 {
            style_of.push(',');
            layer_of.push(',');
            closed.push(',');
            labels.push(',');
        }
        // Every entry is followed by a comma; the closing one is the total, which
        // is what makes `offsets[i]..offsets[i + 1]` work for the last shape too.
        let _ = write!(offsets, "{vertices},");
        for [x, y, z] in &shape.points {
            let _ = write!(
                coords,
                "{},{},{},",
                cm(x - centre[0]),
                cm(y - centre[1]),
                cm(*z)
            );
            vertices += 1;
        }
        let _ = write!(style_of, "{}", shape.style);
        let _ = write!(layer_of, "{}", shape.layer.index());
        let _ = write!(closed, "{}", u8::from(shape.closed));
        let _ = write!(labels, "{}", json_string(&shape.label));
    }
    let _ = write!(offsets, "{vertices}]");
    if coords.ends_with(',') {
        coords.pop();
    }
    coords.push(']');
    style_of.push(']');
    layer_of.push(']');
    closed.push(']');
    labels.push(']');

    let styles = style_table(&scene.styles);
    let styles_light = style_table(&light.styles);

    let mut layers = String::from("[");
    for (index, layer) in ll2_viz::VizLayer::ALL.iter().enumerate() {
        if index > 0 {
            layers.push(',');
        }
        let _ = write!(
            layers,
            "{{\"key\":\"{}\",\"label\":\"{}\",\"on\":{}}}",
            layer.key(),
            layer.label(),
            options.wants_layer(*layer) && *layer != ll2_viz::VizLayer::Centerline
        );
    }
    layers.push(']');

    let s = scene.stats;
    let document = format!(
        "{{\"name\":{},\"projection\":\"{}\",\"coordinates\":\"{}\",\
         \"origin\":[{:.5},{:.5}],\"problems\":{},\
         \"stats\":{{\"lanelets\":{},\"lineStrings\":{},\"polygons\":{},\"areas\":{},\
         \"regulatoryElements\":{},\"points\":{}}},\
         \"bounds\":[{},{},{},{},{},{}],\"background\":\"{}\",\"highlight\":\"{}\",\
         \"layers\":{layers},\"styles\":{styles},\"stylesLight\":{styles_light},\
         \"backgroundLight\":\"{}\",\"highlightLight\":\"{}\",\"centre\":[{},{}],\
         \"offsets\":{offsets},\"coords\":{coords},\"styleOf\":{style_of},\
         \"layerOf\":{layer_of},\"closed\":{closed},\"labels\":{labels}}}",
        json_string(&name),
        loaded.projection,
        loaded.coordinates.key(),
        loaded.origin.lat,
        loaded.origin.lon,
        loaded.problems,
        s.lanelets,
        s.line_strings,
        s.polygons,
        s.areas,
        s.regulatory_elements,
        s.points,
        cm(b.min[0] - centre[0]),
        cm(b.min[1] - centre[1]),
        cm(b.min[2]),
        cm(b.max[0] - centre[0]),
        cm(b.max[1] - centre[1]),
        cm(b.max[2]),
        scene.theme.palette().background.to_hex(),
        scene.theme.palette().highlight.to_hex(),
        ll2_viz::Theme::Light.palette().background.to_hex(),
        ll2_viz::Theme::Light.palette().highlight.to_hex(),
        cm(centre[0]),
        cm(centre[1]),
    );
    std::fs::write(&out_path, &document).unwrap();
    eprintln!(
        "{out_path}: {} shapes, {vertices} vertices, {} KB",
        scene.shapes.len(),
        document.len() / 1024
    );
}

/// The style table for one palette, as JSON.
fn style_table(styles: &ll2_viz::StyleTable) -> String {
    let mut out = String::from("[");
    for (index, style) in styles.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "{{\"label\":{},\"z\":{},\"inLegend\":{},\"strokeWidth\":{},\"hide\":{},\"solid\":{}",
            json_string(style.label),
            style.z,
            style.in_legend,
            style.stroke_width,
            style.hide_below_scale,
            style.solid_below_scale
        );
        match style.stroke {
            Some(c) => {
                let _ = write!(
                    out,
                    ",\"stroke\":\"{}\",\"so\":{}",
                    c.to_hex(),
                    style.stroke_opacity
                );
            }
            None => out.push_str(",\"stroke\":null"),
        }
        match style.fill {
            Some(c) => {
                let _ = write!(
                    out,
                    ",\"fill\":\"{}\",\"fo\":{}",
                    c.to_hex(),
                    style.fill_opacity
                );
            }
            None => out.push_str(",\"fill\":null"),
        }
        match &style.dash {
            Some(d) => {
                let _ = write!(out, ",\"dash\":[{},{}]", d[0], d[1]);
            }
            None => out.push_str(",\"dash\":null"),
        }
        out.push('}');
    }
    out.push(']');
    out
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            // The document is embedded in a <script> block; a bare `<` could end it.
            '<' => out.push_str("\\u003c"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
