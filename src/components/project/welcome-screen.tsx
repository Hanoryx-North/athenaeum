import { useEffect, useState } from "react"
import { FolderOpen, X } from "lucide-react"
import { Button } from "@/components/ui/button"
import { getRecentProjects, removeFromRecentProjects } from "@/lib/project-store"
import type { WikiProject } from "@/types/wiki"
import { CloneWikiDialog } from "./clone-wiki-dialog"
import { LibraryAccountMenu } from "@/components/setup/library-account-menu"

interface WelcomeScreenProps {
  onCreateProject: () => void
  onOpenProject: () => void
  onSelectProject: (project: WikiProject) => void
  onSignedIn: () => Promise<void>
}

export function WelcomeScreen({ onCreateProject, onOpenProject, onSelectProject, onSignedIn }: WelcomeScreenProps) {
  const [recent, setRecent] = useState<WikiProject[]>([])
  const [cloning, setCloning] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    getRecentProjects().then(setRecent).catch(() => setError("Could not load recent projects. Open a folder to continue."))
  }, [])

  async function remove(path: string) {
    try {
      await removeFromRecentProjects(path)
      setRecent(await getRecentProjects())
    } catch { setError("Could not remove this recent project. Try again.") }
  }

  return <main className="relative flex h-full overflow-y-auto bg-background px-6 py-10 text-foreground">
    <div className="absolute right-4 top-4"><LibraryAccountMenu onSignedIn={onSignedIn} /></div>
    <div className="m-auto w-full max-w-md space-y-8">
      <h1 className="text-2xl font-semibold">Open a project</h1>
      <div className="space-y-3">
        <Button className="w-full" onClick={onOpenProject}><FolderOpen className="h-4 w-4" />Open folder</Button>
        <div className="flex justify-center gap-4">
          <Button variant="link" onClick={onCreateProject}>New wiki</Button>
          <Button variant="link" onClick={() => setCloning(true)}>Clone from Library</Button>
        </div>
      </div>
      {recent.length > 0 && <section aria-label="Recent projects">
        <h2 className="mb-2 text-sm text-muted-foreground">Recent projects</h2>
        <ul className="divide-y">
          {recent.map((project) => <li key={project.path} className="flex items-center gap-2">
            <button onClick={() => onSelectProject(project)} className="min-w-0 flex-1 rounded-md px-2 py-3 text-left hover:bg-accent focus-visible:outline-2 focus-visible:outline-ring">
              <span className="block truncate text-sm font-medium">{project.name}</span>
              <span className="block truncate text-xs text-muted-foreground">{project.path}</span>
            </button>
            <Button variant="ghost" size="icon" aria-label={`Remove ${project.name} from recent projects`} onClick={() => void remove(project.path)}><X className="h-4 w-4" /></Button>
          </li>)}
        </ul>
      </section>}
      {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
    </div>
    <CloneWikiDialog open={cloning} onOpenChange={setCloning} onCloned={onSelectProject} />
  </main>
}
