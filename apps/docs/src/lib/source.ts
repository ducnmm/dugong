import { docs } from "collections/server";
import { loader, type Source } from "fumadocs-core/source";
import { icons } from "lucide-react";
import { createElement } from "react";
import { docsRoute } from "./shared";

type DocsSource = Source<{
  pageData: (typeof docs)["docs"][number];
  metaData: (typeof docs)["meta"][number];
}>;

export const source = loader({
  baseUrl: docsRoute,
  source: docs.toFumadocsSource() as DocsSource,
  icon(icon) {
    if (icon && icon in icons) {
      return createElement(icons[icon as keyof typeof icons]);
    }
  }
});
