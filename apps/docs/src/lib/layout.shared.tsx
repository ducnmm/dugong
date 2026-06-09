import type { BaseLayoutProps } from "fumadocs-ui/layouts/shared";
import { dugongAppUrl } from "./shared";

const navTitle = <span className="dugong-wordmark">Dugong Docs</span>;

const layoutLinks: BaseLayoutProps["links"] = [
  {
    text: "Open App",
    url: dugongAppUrl,
    active: "none",
    secondary: false
  }
];

export function docsLayoutOptions(): BaseLayoutProps {
  return {
    links: layoutLinks,
    nav: {
      title: navTitle
    },
    themeSwitch: {
      mode: "light-dark-system"
    }
  };
}
