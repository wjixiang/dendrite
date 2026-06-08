use ratatui::Frame;

use crate::components;
use crate::layout;
use crate::state::{App, Panel};
use crate::agent_panel;

pub fn ui(f: &mut Frame, app: &mut App) {
    let app_layout = layout::compute(f.area());
    let theme = app.theme;

    f.render_stateful_widget(
        components::render_tree(&app.tree_items, app.focused, &theme),
        app_layout.tree_area,
        &mut app.tree_state,
    );
    components::render_knowledge_entity(f, app, &theme, app_layout.ke_area);
    components::render_agent(f, app, &theme, app_layout.agent_area);
    f.render_widget(
        components::render_diagnostics(app, &theme, app_layout.diag_area),
        app_layout.diag_area,
    );
    f.render_widget(
        components::render_help_bar(&theme, app),
        app_layout.help_area,
    );

    // Render the Agents panel when it's focused, using the diag area.
    if app.focused == Panel::Agents {
        agent_panel::render_agent_panel(
            f,
            &app.agent_panel,
            &theme,
            app_layout.diag_area,
            app.spinner_tick,
        );
    }

    if app.settings_modal_open {
        components::render_settings_modal(f, app, &theme);
        if app.new_provider_form.is_some() {
            components::render_new_provider_form(f, app, &theme);
        }
    }

    app.toast.render(f, &theme);
}
