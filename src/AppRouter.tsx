import { useState } from "react";
import App from "./App";
import PiPanel from "./PiPanel";

// The only place that knows both OMP and Pi tools exist. App.tsx and
// PiPanel.tsx are fully self-contained (own state, own theme, own IPC
// bridge) and never import each other — each takes a bare onSwitchTool
// callback instead. Delete this file and one branch below to split Pi
// into its own app; nothing else in either tool needs to change.
type Tool = "omp" | "pi";

export default function AppRouter() {
  const [tool, setTool] = useState<Tool>("omp");
  if (tool === "pi") return <PiPanel onSwitchTool={() => setTool("omp")} />;
  return <App onSwitchTool={() => setTool("pi")} />;
}
