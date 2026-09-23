import { useEffect, useState } from "react";
import Head from "expo-router/head";
import { ViewerStore } from "../src/viewer-store";
import { Workspace } from "../src/workspace";
export default function Home() {
  const [store, setStore] = useState<ViewerStore | null>(null);
  useEffect(() => {
    const next = new ViewerStore();
    setStore(next);
    void next.initialize();
    const close = () => next.stop();
    window.addEventListener("pagehide", close);
    return () => {
      window.removeEventListener("pagehide", close);
      next.stop();
    };
  }, []);
  return (
    <>
      <Head>
        <title>Local Logs — lecteur local</title>
      </Head>
      {store ? (
        <Workspace store={store} />
      ) : (
        <div className="startup">Démarrage du lecteur local…</div>
      )}
    </>
  );
}
