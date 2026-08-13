import { useState } from "react";
import { BoardDetailScreen } from "./screens/BoardDetailScreen.tsx";
import { BoardsScreen } from "./screens/BoardsScreen.tsx";

export function App() {
  // Deliberately no router library — two screens, switched by one piece
  // of state. Adding `react-router` for this would be dependency
  // surface this example doesn't need (see the README's scope note).
  const [selectedBoardId, setSelectedBoardId] = useState<number | null>(null);

  return (
    <div className="page">
      <header className="page-header">
        <span className="page-title">cratestack · react + vite + swr preset</span>
      </header>
      <main className="page-main">
        {selectedBoardId === null ? (
          <BoardsScreen onSelectBoard={setSelectedBoardId} />
        ) : (
          <BoardDetailScreen boardId={selectedBoardId} onBack={() => setSelectedBoardId(null)} />
        )}
      </main>
    </div>
  );
}
