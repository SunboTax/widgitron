import type { ArxivPaper } from "../types/config";
import { tauriInvoke } from "./tauriInvoke";

export async function fetchArxivSavedPapers(): Promise<ArxivPaper[]> {
  return tauriInvoke("get_arxiv_saved_papers");
}

export async function fetchArxivDiscardedPapers(): Promise<ArxivPaper[]> {
  return tauriInvoke("get_arxiv_discarded_papers");
}

export function loadArxivArchiveLists(
  isActive: () => boolean,
  setSaved: (papers: ArxivPaper[]) => void,
  setDiscarded: (papers: ArxivPaper[]) => void
): void {
  fetchArxivSavedPapers()
    .then((papers) => {
      if (isActive()) setSaved(papers);
    })
    .catch(console.error);
  fetchArxivDiscardedPapers()
    .then((papers) => {
      if (isActive()) setDiscarded(papers);
    })
    .catch(console.error);
}
