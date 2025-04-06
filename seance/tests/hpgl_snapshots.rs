#[test]
fn hackspace_logo() {
    let design_file = seance::svg::parse_svg(
        &std::fs::read("../logo.svg").expect("failed to read logo SVG file"),
    )
    .expect("failed to parse logo SVG data");
    let offset = seance::DesignOffset { x: 0.0, y: 0.0 };

    let mut tool_passes = seance::default_passes::default_passes();
    for tool in &mut tool_passes {
        tool.set_enabled(true);
    }

    let paths = seance::svg::get_paths_grouped_by_colour(&design_file);
    let mut paths_in_mm = seance::resolve_paths(&paths, &offset, 1.0);
    seance::paths::filter_paths_to_tool_passes(&mut paths_in_mm, &tool_passes);
    let resolved_paths = seance::paths::convert_points_to_plotter_units(&paths_in_mm);
    let hpgl = seance::hpgl::generate_hpgl(&resolved_paths, &tool_passes);

    insta::assert_snapshot!("york hackspace logo SVG HPGL output", hpgl);
}
