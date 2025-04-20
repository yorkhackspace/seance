#[test]
fn hackspace_logo() {
    let design_file = seance::svg::parse_svg(
        &std::fs::read("../logo.svg").expect("failed to read logo SVG file"),
    )
    .expect("failed to parse logo SVG data");

    let design_name = "York Hackspace Logo";
    let offset = seance::DesignOffset { x: 0.0, y: 0.0 };

    let mut tool_passes = seance::default_passes::default_passes();
    for tool in &mut tool_passes {
        tool.set_enabled(true);
    }

    let bed = &seance::bed::BED_GCC_SPIRIT;

    let paths = seance::svg::get_paths_grouped_by_colour(&design_file);
    insta::assert_debug_snapshot!("york hackspace logo SVG paths", &paths);

    let mut paths_in_mm = seance::resolve_paths(&paths, &offset, 1.0);
    insta::assert_debug_snapshot!("york hackspace logo resolved paths", &paths_in_mm);

    seance::paths::filter_paths_to_tool_passes(&mut paths_in_mm, &tool_passes);
    insta::assert_debug_snapshot!("york hackspace logo filtered paths", &paths_in_mm);

    let resolved_paths = seance::paths::convert_points_to_plotter_units(&paths_in_mm, &bed);
    insta::assert_debug_snapshot!("york hackspace logo plotted paths", &resolved_paths);

    let hpgl = seance::hpgl::generate_hpgl(&resolved_paths, &tool_passes, &bed);
    insta::assert_snapshot!("york hackspace logo HPGL output", &hpgl);

    let pcl = seance::pcl::wrap_hpgl_in_pcl(hpgl, design_name, &tool_passes);
    insta::assert_snapshot!("york hackspace logo PCL output", &pcl);
}
