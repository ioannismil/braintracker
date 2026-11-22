import React from "react";
import ReactDOM from "react-dom/client";
import "./styles.css";

// The phone app has its own layout (src/mobile/); the desktop's is App.
const mobile = /Android|iPhone|iPad/i.test(navigator.userAgent);

async function start() {
  const App = mobile
    ? (await Promise.all([import("./mobile/MobileApp"), import("./mobile/mobile.css")]))[0].default
    : (await import("./App")).default;
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
}
start();
