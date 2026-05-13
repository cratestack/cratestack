// Mirror of the Rust-side `JsNote` / `JsArticle` views. Kept tiny on
// purpose — these are JSON-shaped DTOs over the Tauri IPC channel, and
// any change here should be made to `src-tauri/src/lib.rs` in the same
// commit.

export type Note = {
  id: string;
  title: string;
  body: string;
  pinned: boolean;
  completed: boolean;
  createdAt: string;
  updatedAt: string;
};

export type NewNote = {
  title: string;
  body: string;
  pinned: boolean;
};

export type Article = {
  id: number;
  title: string;
  body: string;
  published: boolean;
  createdAt: string;
};
