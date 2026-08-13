// No `routerProvider` — `<Refine>` only strictly needs `dataProvider`
// (per `@refinedev/core`'s own doc comment on the component). This app
// is a single page with a hand-rolled tab switcher instead of routed
// resource pages; the point being demonstrated is the generated
// dataProvider wiring, not refine's routing integration.
import { Refine } from "@refinedev/core";
import { useState } from "react";
import { dataProvider } from "./dataProvider.js";
import { CategoriesPage } from "./pages/CategoriesPage.js";
import { PostsPage } from "./pages/PostsPage.js";
import { TagsPage } from "./pages/TagsPage.js";

const TABS = [
  { key: "categories", label: "Categories" },
  { key: "posts", label: "Posts" },
  { key: "tags", label: "Tags" },
] as const;

type TabKey = (typeof TABS)[number]["key"];

export function App() {
  const [tab, setTab] = useState<TabKey>("categories");

  return (
    <Refine
      dataProvider={dataProvider}
      resources={[{ name: "categories" }, { name: "posts" }, { name: "tags" }]}
      options={{ disableTelemetry: true }}
    >
      <div className="shell">
        <header className="shell-header">
          <h1>cratestack · refine.dev admin</h1>
          <p className="muted">
            Driven end-to-end by CrateStack codegen (<code>generate-typescript --refine</code> +{" "}
            <code>generate-wiremock</code>) against a generated WireMock backend — no database, no
            hand-written server.
          </p>
          <nav className="tabs">
            {TABS.map((t) => (
              <button
                key={t.key}
                type="button"
                className={t.key === tab ? "tab active" : "tab"}
                onClick={() => setTab(t.key)}
              >
                {t.label}
              </button>
            ))}
          </nav>
        </header>
        <main>
          {tab === "categories" && <CategoriesPage />}
          {tab === "posts" && <PostsPage />}
          {tab === "tags" && <TagsPage />}
        </main>
      </div>
    </Refine>
  );
}
