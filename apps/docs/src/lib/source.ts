import { docs } from "collections/server";
import { loader, type Source } from "fumadocs-core/source";
import { docsRoute } from "./shared";

type DocsSource = Source<{
  pageData: (typeof docs)["docs"][number];
  metaData: (typeof docs)["meta"][number];
}>;

export const source = loader({
  baseUrl: docsRoute,
  source: docs.toFumadocsSource() as DocsSource
});
