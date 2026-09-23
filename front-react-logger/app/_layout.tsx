import { Slot } from "expo-router";
import { GluestackUIProvider } from "../src/ui";
import "../src/styles.css";
export default function Layout() {
  return (
    <GluestackUIProvider>
      <Slot />
    </GluestackUIProvider>
  );
}
