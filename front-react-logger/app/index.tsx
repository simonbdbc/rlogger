import Head from "expo-router/head";
import { Workspace } from "../src/workspace";

export default function Home() {
  return (
    <>
      <Head>
        <title>Local Logs — lecteur local</title>
      </Head>
      <Workspace />
    </>
  );
}
