#[test]
fn logo() {
    let design_file = seance::svg::parse_svg(
        &std::fs::read("../logo.svg").expect("failed to read logo SVG file"),
    )
    .expect("failed to parse logo SVG data");

    let design_name = "Logo";
    let offset = seance::DesignOffset { x: 0.0, y: 0.0 };

    let mut tool_passes = seance::default_passes::default_passes();
    for tool in &mut tool_passes {
        tool.set_enabled(true);
    }

    let paths = seance::svg::get_paths_grouped_by_colour(&design_file);
    let mut paths_in_mm = seance::resolve_paths(&paths, &offset, 1.0);
    seance::paths::filter_paths_to_tool_passes(&mut paths_in_mm, &tool_passes);
    let resolved_paths = seance::paths::convert_points_to_plotter_units(&paths_in_mm);
    insta::assert_debug_snapshot!("logo plotted paths", &resolved_paths);

    let hpgl = seance::hpgl::generate_hpgl(&resolved_paths, &tool_passes);
    insta::assert_snapshot!("logo HPGL output", &hpgl);

    let pcl = seance::pcl::wrap_hpgl_in_pcl(hpgl, design_name, &tool_passes);
    insta::assert_snapshot!("logo PCL output", &pcl);
}

#[test]
fn black_rectangle() {
    let design_file = seance::svg::parse_svg(
        &std::fs::read("tests/black_rectangle.svg").expect("failed to read rectangle SVG file"),
    )
    .expect("failed to parse rectangle SVG data");

    let design_name = "rectangle";
    let offset = seance::DesignOffset { x: 0.0, y: 0.0 };

    let mut tool_passes = seance::default_passes::default_passes();
    for tool in &mut tool_passes {
        tool.set_enabled(true);
    }

    let paths = seance::svg::get_paths_grouped_by_colour(&design_file);
    let mut paths_in_mm = seance::resolve_paths(&paths, &offset, 1.0);
    seance::paths::filter_paths_to_tool_passes(&mut paths_in_mm, &tool_passes);
    let resolved_paths = seance::paths::convert_points_to_plotter_units(&paths_in_mm);
    insta::assert_debug_snapshot!("rectangle plotted paths", &resolved_paths);

    let hpgl = seance::hpgl::generate_hpgl(&resolved_paths, &tool_passes);
    insta::assert_snapshot!("rectangle HPGL output", &hpgl);

    let pcl = seance::pcl::wrap_hpgl_in_pcl(hpgl, design_name, &tool_passes);
    insta::assert_snapshot!("rectangle PCL output", &pcl);
}
