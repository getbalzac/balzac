use std::fmt::Write as _;

use crate::config::RobotsConfig;

/// Generate robots.txt content. Allows everything unless `disallow` rules are configured.
#[must_use]
pub fn generate_robots(config: &RobotsConfig, sitemap_url: Option<&str>) -> String {
    let mut robots = String::from("User-agent: *\n");

    if config.disallow.is_empty() {
        robots.push_str("Allow: /\n");
    } else {
        for path in &config.disallow {
            let _ = writeln!(robots, "Disallow: {path}");
        }
    }

    if let Some(url) = sitemap_url {
        let _ = write!(robots, "\nSitemap: {url}\n");
    }

    robots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_robots_default_allows_all() {
        let robots = generate_robots(&RobotsConfig::default(), None);
        assert_eq!(robots, "User-agent: *\nAllow: /\n");
    }

    #[test]
    fn test_generate_robots_with_disallow_and_sitemap() {
        let config = RobotsConfig {
            disallow: vec!["/drafts/".to_string(), "/private".to_string()],
            ..Default::default()
        };
        let robots = generate_robots(&config, Some("https://example.com/sitemap.xml"));
        assert_eq!(
            robots,
            "User-agent: *\nDisallow: /drafts/\nDisallow: /private\n\nSitemap: https://example.com/sitemap.xml\n"
        );
    }
}
