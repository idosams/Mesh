import React from "react";
import { createRoot } from "react-dom/client";
import { GalleryPage } from "./pages/gallery-page";
import "./styles.css";

const root = document.getElementById("root");

if (!root) {
  throw new Error("Mesh UI root is missing");
}

createRoot(root).render(
  <React.StrictMode>
    <GalleryPage />
  </React.StrictMode>,
);
