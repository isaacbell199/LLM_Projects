'use client'

import { useState, useRef, useEffect, useCallback } from 'react'
import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import {
  Loader2,
  Wand2,
  Square,
  X,
  Zap,
  AlertCircle,
  GripVertical,
  RefreshCw,
  CheckCircle,
  Play,
  ArrowRight,
  BookOpen,
  Compass
} from 'lucide-react'
import { motion, AnimatePresence, Reorder } from 'framer-motion'
import { toast } from 'sonner'
import { useStore } from '@/lib/store'
import {
  generateText,
  stopGeneration,
  onGenerationChunk,
  isTauri
} from '@/lib/tauri-api'

// ============================================================================
// Types
// ============================================================================

type TabType = 'story' | 'action'

interface CharacterData {
  id: string
  name: string
  description: string
  isFromWorld?: boolean
}

interface StorySituation {
  id: string
  text: string
  generatedParagraph: string
  status: 'pending' | 'generating' | 'done' | 'error'
}

// ============================================================================
// Action Tab - Visceral Narration Buttons
// ============================================================================

const ACTION_BUTTONS = [
  {
    id: 'physical-impact',
    name: 'PHYSICAL IMPACT',
    focus: 'Raw contact, matter, fluids.',
    instruction: 'Describe the shock of bodies or objects. Include blood splatter, sweat, saliva, flesh tearing, or the softness of a caress. Detail the sounds (cracking, rustling, moaning).',
    color: 'rose'
  },
  {
    id: 'internal-sensations',
    name: 'INTERNAL SENSATIONS',
    focus: 'The character\'s biological system.',
    instruction: 'Describe what cannot be seen from outside: vibrations in bones, burning of wounds, pounding heart, heat rising (blushing), adrenaline, dizziness, pleasures, joys, fears.',
    color: 'amber'
  },
  {
    id: 'expression-cry',
    name: 'EXPRESSION / CRY',
    focus: 'Vocal output and facial expression.',
    instruction: 'Generate spoken words, cries, groans, or murmurs. Describe precisely the expression of eyes, facial contractions, body movements.',
    color: 'sky'
  },
  {
    id: 'scene-atmosphere',
    name: 'SCENE & ATMOSPHERE',
    focus: 'Environmental immersion.',
    instruction: 'Describe characters, their positions, facial and physical expressions (pleasures, joys, fears), smells, sounds, noises (metallic, perfume, dust), light, wind, heat, or oppressive silence.',
    color: 'emerald'
  },
  {
    id: 'secret-thought',
    name: 'SECRET THOUGHT',
    focus: 'Inner monologue (the unspoken).',
    instruction: 'Reveal what the character truly thinks, their doubts, hidden desires, or strategies, but keeps strictly to themselves.',
    color: 'violet'
  },
]

// ============================================================================
// Component
// ============================================================================

interface AIAssistantProps {
  editorContent: string
  onStreamChunk: (chunk: string) => void
  isGenerating: boolean
  onGenerationStart: () => void
  onGenerationEnd: () => void
  creativeCategory?: string
  creativeTone?: string
  creativeStyle?: string
  creativeTheme?: string
  projectCharacters?: { id: string; name: string; description: string }[]
  projectLore?: string
}

export function AIAssistant({
  editorContent,
  onStreamChunk,
  isGenerating,
  onGenerationStart,
  onGenerationEnd,
  creativeCategory,
  creativeTone,
  creativeStyle,
  creativeTheme,
  projectCharacters = [],
  projectLore,
}: AIAssistantProps) {
  const { isModelLoaded, temperature, outputLanguage } = useStore()

  // Tab state - Default to 'action'
  const [activeTab, setActiveTab] = useState<TabType>('action')

  // ============ SHARED STATE ============
  const [characters, setCharacters] = useState<CharacterData[]>([])
  const currentGeneratingIndexRef = useRef<number>(-1)

  // ============ STORY TAB STATE ============
  const [storyInputText, setStoryInputText] = useState('')
  const [storySituations, setStorySituations] = useState<StorySituation[]>([])
  const [currentGeneratingIndex, setCurrentGeneratingIndex] = useState<number>(-1)
  const [storyMode, setStoryMode] = useState<'input' | 'situations' | 'generating'>('input')

  // ============ ACTION TAB STATE ============
  const [actionSelectedActor, setActionSelectedActor] = useState<string>('narrator')
  const [actionDescription, setActionDescription] = useState('')
  const [actionPhraseCount, setActionPhraseCount] = useState(2)
  const [actionAutoMode, setActionAutoMode] = useState(true)

  // ============ STORY RAIL STATE ============
  const [storyRailContent, setStoryRailContent] = useState<string>('')
  const [showStoryRail, setShowStoryRail] = useState(false)

  // Load characters from project, World Studio, and Database
  useEffect(() => {
    async function loadAllCharacters() {
      const initialChars: CharacterData[] = projectCharacters.map(c => ({
        ...c,
        isFromWorld: false
      }))

      // Load from World Studio (localStorage)
      try {
        const savedWorld = localStorage.getItem('world_studio_current')
        if (savedWorld) {
          const worldData = JSON.parse(savedWorld)
          if (worldData.characters && Array.isArray(worldData.characters)) {
            const worldChars = worldData.characters
              .filter((c: { title: string }) => c.title?.trim())
              .map((c: { id: string; title: string; description: string }) => ({
                id: c.id,
                name: c.title,
                description: c.description || '',
                isFromWorld: true
              }))
            initialChars.push(...worldChars)
          }
        }
      } catch (e) {
        console.error('Failed to load world data:', e)
      }

      // 100% Desktop - All data loaded from Tauri backend via localStorage
      // Characters are stored in localStorage by World Studio

      // Filter out any character named exactly "Narrator" to avoid duplicates
      const filteredChars = initialChars.filter(c => c.name !== 'Narrator')
      setCharacters(filteredChars)
    }

    loadAllCharacters()
  }, [projectCharacters])

  // Load Story Rail content from localStorage on mount
  useEffect(() => {
    try {
      const savedRail = localStorage.getItem('story_rail_content')
      if (savedRail) {
        setStoryRailContent(savedRail)
      }
    } catch (e) {
      console.error('Failed to load story rail:', e)
    }
  }, [])

  // Save Story Rail content
  const handleSaveStoryRail = useCallback(() => {
    try {
      localStorage.setItem('story_rail_content', storyRailContent)
      setShowStoryRail(false)
      toast.success('Story Rail saved!')
    } catch (e) {
      console.error('Failed to save story rail:', e)
      toast.error('Failed to save Story Rail')
    }
  }, [storyRailContent])

  // Helper: Build creative settings prompt suffix
  const buildCreativePrompt = useCallback(() => {
    let prompt = ''
    if (creativeCategory) prompt += `Genre: ${creativeCategory}\n`
    if (creativeTone) prompt += `Tone: ${creativeTone}\n`
    if (creativeStyle) prompt += `Writing Style: ${creativeStyle}\n`
    if (creativeTheme) prompt += `Theme: ${creativeTheme}\n`
    if (projectLore) prompt += `World Lore: ${projectLore}\n`
    return prompt
  }, [creativeCategory, creativeTone, creativeStyle, creativeTheme, projectLore])

  // Stop generation
  const handleStopGeneration = useCallback(() => {
    stopGeneration()
    onGenerationEnd()
    setCurrentGeneratingIndex(-1)
    toast.info('Generation stopped')
  }, [onGenerationEnd])

  // ============ STORY FUNCTIONS ============

  const parseSituations = useCallback(() => {
    const lines = storyInputText
      .split('\n')
      .map(line => line.trim())
      .filter(line => line.length > 0)
    
    if (lines.length === 0) {
      toast.error('Please enter at least one situation')
      return
    }

    const situations: StorySituation[] = lines.map((text, index) => ({
      id: `sit-${Date.now()}-${index}`,
      text,
      generatedParagraph: '',
      status: 'pending'
    }))

    setStorySituations(situations)
    setStoryMode('situations')
    toast.success(`${situations.length} situations ready!`)
  }, [storyInputText])

  const handleReorderSituations = useCallback((newOrder: StorySituation[]) => {
    setStorySituations(newOrder)
  }, [])

  const removeSituation = useCallback((id: string) => {
    setStorySituations(prev => prev.filter(s => s.id !== id))
  }, [])

  const generateStory = async () => {
    if (!isModelLoaded) {
      toast.error('Please load a model first')
      return
    }

    if (storySituations.length === 0) {
      toast.error('No situations to generate')
      return
    }

    setStoryMode('generating')
    onGenerationStart()

    try {
      const resetSituations = storySituations.map(s => ({
        ...s,
        status: 'pending' as const,
        generatedParagraph: ''
      }))
      setStorySituations(resetSituations)

      let generatedSoFar = ''

      for (let i = 0; i < resetSituations.length; i++) {
        if (!isGenerating && i > 0) break

        setCurrentGeneratingIndex(i)
        currentGeneratingIndexRef.current = i
        
        setStorySituations(prev => prev.map((s, idx) => 
          idx === i ? { ...s, status: 'generating' } : s
        ))

        const situation = resetSituations[i]
        const creativePrompt = buildCreativePrompt()
        const previousContext = generatedSoFar 
          ? `\n\nPREVIOUSLY IN THE STORY:\n${generatedSoFar.slice(-1500)}\n` 
          : ''

        // Build Story Rail section if content exists
        const storyRailSection = storyRailContent.trim() 
          ? `\n[FRAMEWORK INSTRUCTIONS - STORY RAIL]\nHere is the complete story path: "${storyRailContent}"\nYou must use this text ONLY as a reference for coherence, not as content to copy.\n` 
          : ''

        const prompt = `${creativePrompt}${storyRailSection}
You are writing paragraph ${i + 1} of ${resetSituations.length} in a continuous story.

SITUATION TO WRITE: ${situation.text}
${previousContext}
RULES:
- Write ONE paragraph (2-4 sentences) for this specific situation
- If this is not the first paragraph, connect smoothly to the previous events
- Maintain consistent tone, style, and narrative voice
- Be vivid and engaging

[CRITICAL BLOCKING CONSTRAINT]
Absolute prohibition on anticipating the scenario continuation.
Never skip steps, never summarize, never anticipate.
Focus exclusively on the current action requested.

Write the paragraph now:`

        if (isTauri()) {
          let paragraphContent = ''

          const unsubscribe = onGenerationChunk((chunk) => {
            if (chunk.done) {
              setStorySituations(prev => prev.map((s, idx) => 
                idx === i ? { ...s, status: 'done', generatedParagraph: paragraphContent } : s
              ))
              
              if (i === 0) {
                onStreamChunk(paragraphContent)
              } else {
                onStreamChunk('\n\n' + paragraphContent)
              }
              
              generatedSoFar += paragraphContent + '\n\n'
            } else {
              paragraphContent += chunk.content
            }
          })

          await generateText({
            mode: 'story',
            text: prompt,
            context: editorContent.slice(-1000) + generatedSoFar.slice(-1000),
            temperature,
            maxTokens: 300,
            language: outputLanguage,
            selectedTone: creativeTone,
            customStyleName: creativeStyle,
            customGenreName: creativeCategory,
            customStyleInstruction: creativeTheme,
          })

          setTimeout(() => unsubscribe(), 30000)
          await new Promise(r => setTimeout(r, 300))
        } else {
          await new Promise(r => setTimeout(r, 800))
          const demoParagraph = `[Para ${i + 1}] ${situation.text}... The narrative unfolds with vivid details and engaging prose.`
          
          setStorySituations(prev => prev.map((s, idx) => 
            idx === i ? { ...s, status: 'done', generatedParagraph: demoParagraph } : s
          ))
          
          if (i === 0) {
            onStreamChunk(demoParagraph)
          } else {
            onStreamChunk('\n\n' + demoParagraph)
          }
          
          generatedSoFar += demoParagraph + '\n\n'
        }
      }

      setCurrentGeneratingIndex(-1)
      currentGeneratingIndexRef.current = -1
      onGenerationEnd()
      toast.success('Story completed!')
      
    } catch (error) {
      console.error('Story error:', error)
      onGenerationEnd()
      const failedIndex = currentGeneratingIndexRef.current
      setCurrentGeneratingIndex(-1)
      currentGeneratingIndexRef.current = -1
      
      if (failedIndex >= 0) {
        setStorySituations(prev => prev.map((s, idx) => 
          idx === failedIndex ? { ...s, status: 'error' } : s
        ))
      }
      
      toast.error('Generation failed')
    }
  }

  const regenerateParagraph = async (index: number) => {
    if (!isModelLoaded || isGenerating) return

    const situation = storySituations[index]
    if (!situation) return

    setCurrentGeneratingIndex(index)
    onGenerationStart()

    setStorySituations(prev => prev.map((s, idx) => 
      idx === index ? { ...s, status: 'generating', generatedParagraph: '' } : s
    ))

    try {
      const previousParagraphs = storySituations
        .slice(0, index)
        .filter(s => s.status === 'done')
        .map(s => s.generatedParagraph)
        .join('\n\n')

      const creativePrompt = buildCreativePrompt()
      
      const prompt = `${creativePrompt}
You are rewriting paragraph ${index + 1} of a story.

SITUATION TO WRITE: ${situation.text}
${previousParagraphs ? `\nPREVIOUS CONTEXT:\n${previousParagraphs.slice(-1000)}\n` : ''}
RULES:
- Write ONE paragraph (2-4 sentences) for this specific situation
- Connect smoothly to any previous context
- Maintain consistent tone and style
- Be vivid and engaging

Write the paragraph now:`

      let paragraphContent = ''

      if (isTauri()) {
        const unsubscribe = onGenerationChunk((chunk) => {
          if (chunk.done) {
            setStorySituations(prev => prev.map((s, idx) => 
              idx === index ? { ...s, status: 'done', generatedParagraph: paragraphContent } : s
            ))
            setCurrentGeneratingIndex(-1)
            onGenerationEnd()
            toast.success('Paragraph regenerated!')
          } else {
            paragraphContent += chunk.content
          }
        })

        await generateText({
          mode: 'story',
          text: prompt,
          context: editorContent.slice(-1000),
          temperature,
          maxTokens: 300,
          language: outputLanguage,
          selectedTone: creativeTone,
          customStyleName: creativeStyle,
          customGenreName: creativeCategory,
          customStyleInstruction: creativeTheme,
        })

        setTimeout(() => unsubscribe(), 30000)
      } else {
        await new Promise(r => setTimeout(r, 800))
        const demoParagraph = `[Regenerated] ${situation.text}... Fresh narrative with new perspectives.`
        
        setStorySituations(prev => prev.map((s, idx) => 
          idx === index ? { ...s, status: 'done', generatedParagraph: demoParagraph } : s
        ))
        setCurrentGeneratingIndex(-1)
        onGenerationEnd()
        toast.success('Paragraph regenerated! (Demo)')
      }
    } catch (error) {
      console.error('Regenerate error:', error)
      setStorySituations(prev => prev.map((s, idx) => 
        idx === index ? { ...s, status: 'error' } : s
      ))
      setCurrentGeneratingIndex(-1)
      onGenerationEnd()
      toast.error('Regeneration failed')
    }
  }

  const insertStoryToEditor = () => {
    const fullStory = storySituations
      .filter(s => s.status === 'done')
      .map(s => s.generatedParagraph)
      .join('\n\n')
    
    if (fullStory) {
      onStreamChunk('\n\n' + fullStory)
      toast.success('Story inserted!')
    }
  }

  const resetStory = () => {
    setStoryMode('input')
    setStorySituations([])
    setCurrentGeneratingIndex(-1)
  }

  // ============ ACTION TAB FUNCTIONS ============

  const generateActionContent = async (actionButtonId: string) => {
    if (!isModelLoaded) {
      toast.error('Please load a model first')
      return
    }

    if (!actionDescription.trim()) {
      toast.error('Please enter a description')
      return
    }

    const actionButton = ACTION_BUTTONS.find(b => b.id === actionButtonId)
    if (!actionButton) return

    const actorName = actionSelectedActor === 'narrator' 
      ? 'Narrator' 
      : characters.find(c => c.id === actionSelectedActor)?.name || 'Narrator'

    const detectContext = () => {
      const text = actionDescription.toLowerCase()
      const violentKeywords = ['frappe', 'coup', 'bless', 'sang', 'mort', 'combat', 'attaque', 'déchire', 'lame', 'poing', 'tuer', 'battle', 'hit', 'cut', 'blood', 'kill', 'fight', 'punch', 'slash']
      const romanticKeywords = ['caresse', 'embrasse', 'amour', 'désir', 'lèvres', 'tendresse', 'doucement', 'touch', 'love', 'kiss', 'caress', 'embrace', 'gentle']
      
      const hasViolent = violentKeywords.some(k => text.includes(k))
      const hasRomantic = romanticKeywords.some(k => text.includes(k))
      
      if (hasViolent && !hasRomantic) return 'violent'
      if (hasRomantic && !hasViolent) return 'romantic'
      return 'neutral'
    }

    const contextType = actionAutoMode ? detectContext() : 'neutral'

    onGenerationStart()

    try {
      if (isTauri()) {
        const unsubscribe = onGenerationChunk((chunk) => {
          if (chunk.done) {
            onGenerationEnd()
            toast.success(`${actionButton.name} generated!`)
          } else {
            onStreamChunk(chunk.content)
          }
        })

        let styleInstruction = ''
        if (actionAutoMode) {
          if (contextType === 'violent') {
            styleInstruction = 'Style: Use raw, graphic, and visceral language. Be unflinching in depicting physical reality.'
          } else if (contextType === 'romantic') {
            styleInstruction = 'Style: Use poetic, sensory, and intimate language. Focus on tenderness and emotional depth.'
          } else {
            styleInstruction = 'Style: Use vivid and engaging language appropriate to the scene.'
          }
        } else {
          styleInstruction = 'Style: Use neutral, descriptive language.'
        }

        const creativePrompt = buildCreativePrompt()

        // Build Story Rail section if content exists
        const storyRailSection = storyRailContent.trim() 
          ? `\n[FRAMEWORK INSTRUCTIONS - STORY RAIL]\nHere is the complete story path: "${storyRailContent}"\nYou must use this text ONLY as a reference for coherence, not as content to copy.\n` 
          : ''

        const prompt = `${creativePrompt}${storyRailSection}

You are a Visceral Narration Engine. Your role is to generate ultra-precise narrative segments based on action buttons.

GOLDEN RULE: Never summarize. Never anticipate the next action. Respond strictly according to the button clicked and the number of sentences chosen.

ACTOR: ${actorName}
${actionSelectedActor !== 'narrator' ? `CHARACTER: ${actorName} performs the action.` : 'NARRATOR: The narrator describes the action.'}

[ACTION TO GENERATE NOW]
Subject: ${actorName}
Action: ${actionDescription}

ACTION BUTTON: ${actionButton.name}
FOCUS: ${actionButton.focus}
INSTRUCTION: ${actionButton.instruction}

LENGTH CONSTRAINT: Exactly ${actionPhraseCount} sentence${actionPhraseCount > 1 ? 's' : ''}.

${styleInstruction}

STORY CONTEXT:
${editorContent.slice(-1500)}

[CRITICAL BLOCKING CONSTRAINT]
Absolute prohibition on anticipating the scenario continuation.
Never skip steps, never summarize, never anticipate.
Focus exclusively on the current action requested.

RESPONSE (exactly ${actionPhraseCount} sentence${actionPhraseCount > 1 ? 's' : ''}):`

        await generateText({
          mode: 'action',
          text: prompt,
          context: editorContent.slice(-1500),
          temperature: contextType === 'violent' ? 0.85 : contextType === 'romantic' ? 0.88 : temperature,
          maxTokens: 300,
          language: outputLanguage,
          selectedTone: creativeTone,
          customStyleName: creativeStyle,
          customGenreName: creativeCategory,
          customStyleInstruction: creativeTheme,
        })

        setTimeout(() => unsubscribe(), 60000)
      } else {
        await new Promise(r => setTimeout(r, 1000))
        const demoResponse = `[${actionButton.name}] ${actorName}: ${actionDescription}... The visceral narrative unfolds with intense detail.`
        onStreamChunk(demoResponse)
        onGenerationEnd()
        toast.success(`${actionButton.name} generated! (Demo)`)
      }
    } catch (error) {
      console.error('Action generation error:', error)
      onGenerationEnd()
      toast.error('Generation failed')
    }
  }

  // Tab definitions
  const tabs = [
    { id: 'action', label: 'Action', icon: Zap },
    { id: 'story', label: 'Story', icon: BookOpen },
  ] as const

  return (
    <div className="flex flex-col h-[260px] border border-border/50 rounded-xl bg-gradient-to-b from-background to-muted/20 overflow-hidden">
      {/* Tabs */}
      <div className="flex border-b border-border/30 bg-muted/10 overflow-x-auto shrink-0 h-9">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={cn(
              "flex items-center justify-center gap-1.5 px-4 py-2 text-xs font-medium transition-all relative whitespace-nowrap flex-1",
              activeTab === tab.id
                ? "text-violet-600 dark:text-violet-400 bg-violet-500/10"
                : "text-muted-foreground hover:text-foreground hover:bg-muted/30"
            )}
          >
            <tab.icon className="h-3.5 w-3.5" />
            {tab.label}
            {activeTab === tab.id && (
              <motion.div
                layoutId="activeTabAIAssistant"
                className="absolute bottom-0 left-0 right-0 h-0.5 bg-gradient-to-r from-violet-500 to-fuchsia-500"
              />
            )}
          </button>
        ))}
      </div>

      {/* Content */}
      <div className="flex-1 overflow-hidden relative">
        <AnimatePresence mode="wait">
          {/* ============ ACTION TAB ============ */}
          {activeTab === 'action' && (
            <motion.div
              key="action"
              initial={{ opacity: 0, x: -10 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: 10 }}
              className="absolute inset-0 overflow-hidden p-2"
            >
              <div className="flex gap-2 h-full">
                {/* LEFT SECTION - Actor Selection */}
                <div className="flex flex-col gap-1.5 w-[45%]">
                  {/* Narrator Button */}
                  <button
                    onClick={() => setActionSelectedActor('narrator')}
                    className={cn(
                      "h-8 px-2 rounded text-[10px] font-medium border transition-all text-left",
                      actionSelectedActor === 'narrator'
                        ? "bg-emerald-500/30 border-emerald-400 text-emerald-700 dark:text-emerald-300"
                        : "border-border/50 bg-muted/20 hover:bg-muted/40 text-muted-foreground"
                    )}
                  >
                    Narrator
                  </button>

                  {/* Character Buttons */}
                  <div className="flex-1 overflow-y-auto space-y-1 pr-1">
                    {characters.slice(0, 5).map((char) => (
                      <button
                        key={char.id}
                        onClick={() => setActionSelectedActor(char.id)}
                        className={cn(
                          "w-full h-8 px-2 rounded text-[10px] font-medium border transition-all text-left truncate",
                          actionSelectedActor === char.id
                            ? "bg-emerald-500/30 border-emerald-400 text-emerald-700 dark:text-emerald-300"
                            : "border-border/50 bg-muted/20 hover:bg-muted/40 text-muted-foreground"
                        )}
                      >
                        {char.name}
                      </button>
                    ))}
                    {characters.length === 0 && (
                      <div className="text-[9px] text-muted-foreground text-center py-2 italic">
                        No characters available
                      </div>
                    )}
                  </div>

                  {/* Description Input */}
                  <div className="shrink-0">
                    <Textarea
                      placeholder="Write a phrase to enrich..."
                      value={actionDescription}
                      onChange={(e) => setActionDescription(e.target.value)}
                      className="min-h-[48px] max-h-[60px] resize-none text-xs"
                    />
                  </div>
                </div>

                {/* RIGHT SECTION - Action Buttons */}
                <div className="flex flex-col gap-1.5 flex-1">
                  {/* Control Buttons */}
                  <div className="flex gap-1.5 shrink-0 relative">
                    {/* Story Rail Compass Button */}
                    <div className="relative">
                      <button
                        onClick={() => setShowStoryRail(!showStoryRail)}
                        className={cn(
                          "w-7 h-7 rounded text-[10px] font-medium border transition-all flex items-center justify-center",
                          showStoryRail
                            ? "bg-violet-500/30 border-violet-400 text-violet-700 dark:text-violet-300"
                            : "border-border/50 bg-muted/20 hover:bg-muted/40 text-muted-foreground"
                        )}
                        title="Story Rail"
                      >
                        <Compass className="h-4 w-4" />
                      </button>
                      {/* Green indicator when Rail has content */}
                      {storyRailContent.trim() && (
                        <span className="absolute -top-0.5 -right-0.5 w-2 h-2 bg-green-500 rounded-full border border-background" />
                      )}
                    </div>

                    {/* Story Rail Popup */}
                    {showStoryRail && (
                      <div className="absolute top-[-200px] left-0 w-[280px] z-50 bg-background border border-border/50 rounded-lg shadow-lg overflow-hidden">
                        {/* Header */}
                        <div className="flex items-center justify-between px-3 py-2 border-b border-border/30 bg-muted/10">
                          <span className="text-[10px] font-medium text-foreground">Story Master Rail</span>
                          <button
                            onClick={() => setShowStoryRail(false)}
                            className="w-5 h-5 rounded flex items-center justify-center hover:bg-muted/40 text-muted-foreground"
                          >
                            <X className="h-3 w-3" />
                          </button>
                        </div>
                        {/* Body */}
                        <div className="p-2">
                          <Textarea
                            value={storyRailContent}
                            onChange={(e) => setStoryRailContent(e.target.value)}
                            placeholder="Enter your story path here...\n\nThis will be used as a reference for coherence in both Action and Story generation."
                            className="min-h-[140px] max-h-[160px] resize-none text-xs"
                          />
                        </div>
                        {/* Footer */}
                        <div className="flex items-center justify-between px-3 py-2 border-t border-border/30 bg-muted/10">
                          <span className="text-[9px] text-muted-foreground">
                            {storyRailContent.split('\n').filter(l => l.trim()).length} lines
                          </span>
                          <Button
                            onClick={handleSaveStoryRail}
                            size="sm"
                            className="h-6 text-[9px] px-3 bg-gradient-to-r from-violet-500 to-fuchsia-500 border-0"
                          >
                            SAVE
                          </Button>
                        </div>
                      </div>
                    )}

                    <button
                      onClick={() => setActionAutoMode(!actionAutoMode)}
                      className={cn(
                        "flex-1 h-7 px-2 rounded text-[9px] font-medium border transition-all",
                        actionAutoMode
                          ? "bg-emerald-500/30 border-emerald-400 text-emerald-700 dark:text-emerald-300"
                          : "border-border/50 bg-muted/20 text-muted-foreground"
                      )}
                    >
                      {actionAutoMode ? 'AUTO ON' : 'AUTO OFF'}
                    </button>

                    <div className="flex gap-0.5">
                      {[1, 2, 3, 4, 5].map((count) => (
                        <button
                          key={count}
                          onClick={() => setActionPhraseCount(count)}
                          className={cn(
                            "w-6 h-7 rounded text-[9px] font-medium border transition-all",
                            actionPhraseCount === count
                              ? "bg-violet-500/30 border-violet-400 text-violet-700 dark:text-violet-300"
                              : "border-border/50 bg-muted/20 text-muted-foreground"
                          )}
                        >
                          {count}
                        </button>
                      ))}
                    </div>
                  </div>

                  {/* Action Buttons */}
                  <div className="flex-1 flex flex-col gap-1 overflow-y-auto">
                    {ACTION_BUTTONS.map((action) => {
                      // Get color classes based on action color
                      const getColorClasses = (color: string, isGeneratingState: boolean) => {
                        if (isGeneratingState) {
                          return "bg-red-500/30 border-red-400 text-red-700 dark:text-red-300"
                        }
                        switch (color) {
                          case 'rose':
                            return "bg-rose-500/20 border-rose-400 hover:bg-rose-500/30 text-rose-700 dark:text-rose-300"
                          case 'amber':
                            return "bg-amber-500/20 border-amber-400 hover:bg-amber-500/30 text-amber-700 dark:text-amber-300"
                          case 'sky':
                            return "bg-sky-500/20 border-sky-400 hover:bg-sky-500/30 text-sky-700 dark:text-sky-300"
                          case 'emerald':
                            return "bg-emerald-500/20 border-emerald-400 hover:bg-emerald-500/30 text-emerald-700 dark:text-emerald-300"
                          case 'violet':
                            return "bg-violet-500/20 border-violet-400 hover:bg-violet-500/30 text-violet-700 dark:text-violet-300"
                          default:
                            return "bg-muted/20 border-border/50 hover:bg-muted/40 text-muted-foreground"
                        }
                      }
                      
                      return (
                        <button
                          key={action.id}
                          onClick={() => isGenerating ? handleStopGeneration() : generateActionContent(action.id)}
                          disabled={!isModelLoaded || !actionDescription.trim()}
                          className={cn(
                            "w-full h-9 px-2 rounded text-[10px] font-medium border transition-all text-left",
                            getColorClasses(action.color, isGenerating)
                          )}
                        >
                          {action.name}
                        </button>
                      )
                    })}
                  </div>

                  {/* Generate Button */}
                  <Button
                    onClick={() => isGenerating ? handleStopGeneration() : generateActionContent(ACTION_BUTTONS[0].id)}
                    disabled={!isModelLoaded || !actionDescription.trim()}
                    className={cn(
                      "w-full h-7 text-[10px] font-medium border-0 shrink-0",
                      isGenerating
                        ? "bg-gradient-to-r from-red-500 to-rose-500"
                        : "bg-gradient-to-r from-violet-500 to-fuchsia-500"
                    )}
                  >
                    {isGenerating ? (
                      <>
                        <Square className="h-3 w-3 mr-1" />
                        Stop
                      </>
                    ) : (
                      <>
                        <Wand2 className="h-3 w-3 mr-1" />
                        Generate
                      </>
                    )}
                  </Button>
                </div>
              </div>
            </motion.div>
          )}

          {/* ============ STORY TAB ============ */}
          {activeTab === 'story' && (
            <motion.div
              key="story"
              initial={{ opacity: 0, x: -10 }}
              animate={{ opacity: 1, x: 0 }}
              exit={{ opacity: 0, x: 10 }}
              className="absolute inset-0 overflow-y-auto p-3"
            >
              <div className="space-y-2">
                {/* INPUT MODE */}
                {storyMode === 'input' && (
                  <>
                    <div>
                      <div className="flex items-center justify-between mb-1">
                        <span className="text-[10px] font-medium text-muted-foreground">Story Situations</span>
                        <span className="text-[9px] text-muted-foreground">One per line</span>
                      </div>
                      <Textarea
                        placeholder="Enter each situation on a new line:

The hero enters the dark forest
A mysterious figure appears
They exchange cryptic words
The figure vanishes into shadows
The hero continues deeper..."
                        value={storyInputText}
                        onChange={(e) => setStoryInputText(e.target.value)}
                        className="min-h-[120px] resize-none text-xs"
                      />
                      {storyInputText.trim() && (
                        <div className="mt-1 flex items-center gap-1 text-[9px] text-muted-foreground">
                          <span className="px-1.5 py-0.5 rounded bg-muted/30 border border-border/30">
                            {storyInputText.split('\n').filter(line => line.trim()).length} situations
                          </span>
                        </div>
                      )}
                    </div>

                    <div className="flex gap-2">
                      <Button
                        onClick={parseSituations}
                        disabled={!storyInputText.trim()}
                        className="flex-1 h-8 text-xs bg-gradient-to-r from-violet-500 to-fuchsia-500 border-0"
                      >
                        <Play className="h-3 w-3 mr-1" />
                        Preview & Edit
                      </Button>
                    </div>
                  </>
                )}

                {/* SITUATIONS MODE */}
                {storyMode === 'situations' && (
                  <>
                    <div className="flex items-center justify-between">
                      <span className="text-[10px] font-medium text-muted-foreground">
                        {storySituations.length} Situations (drag to reorder)
                      </span>
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={resetStory}
                        className="h-6 text-[10px] px-2"
                      >
                        <X className="h-3 w-3 mr-1" />
                        Reset
                      </Button>
                    </div>

                    <Reorder.Group
                      axis="y"
                      values={storySituations}
                      onReorder={handleReorderSituations}
                      className="space-y-1.5"
                    >
                      {storySituations.map((situation, index) => (
                        <Reorder.Item
                          key={situation.id}
                          value={situation}
                          className="flex items-center gap-2 p-2 rounded-lg bg-muted/30 border border-border/30 cursor-grab active:cursor-grabbing"
                        >
                          <GripVertical className="h-3.5 w-3.5 text-muted-foreground shrink-0" />
                          <span className="text-[10px] font-medium text-muted-foreground w-4">{index + 1}.</span>
                          <span className="text-xs flex-1 truncate">{situation.text}</span>
                          <Button
                            variant="ghost"
                            size="icon"
                            onClick={(e) => {
                              e.stopPropagation()
                              removeSituation(situation.id)
                            }}
                            className="h-5 w-5 shrink-0"
                          >
                            <X className="h-3 w-3" />
                          </Button>
                        </Reorder.Item>
                      ))}
                    </Reorder.Group>

                    <div className="flex gap-2">
                      <Button
                        onClick={generateStory}
                        disabled={!isModelLoaded || storySituations.length === 0}
                        className="flex-1 h-8 text-xs bg-gradient-to-r from-violet-500 to-fuchsia-500 border-0"
                      >
                        <Play className="h-3 w-3 mr-1" />
                        Generate Story
                      </Button>
                    </div>
                  </>
                )}

                {/* GENERATING MODE */}
                {storyMode === 'generating' && (
                  <>
                    <div className="flex items-center justify-between">
                      <span className="text-[10px] font-medium text-muted-foreground">
                        Generating ({currentGeneratingIndex + 1}/{storySituations.length})
                      </span>
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={handleStopGeneration}
                        className="h-6 text-[10px] px-2 text-red-500 hover:text-red-600"
                      >
                        <Square className="h-3 w-3 mr-1" />
                        Stop
                      </Button>
                    </div>

                    {/* Progress Bar */}
                    <div className="w-full h-1.5 bg-muted rounded-full overflow-hidden">
                      <motion.div
                        className="h-full bg-gradient-to-r from-violet-500 to-fuchsia-500"
                        initial={{ width: 0 }}
                        animate={{ width: `${((currentGeneratingIndex + 1) / storySituations.length) * 100}%` }}
                        transition={{ duration: 0.3 }}
                      />
                    </div>

                    {/* Situation Status List */}
                    <div className="space-y-1 max-h-[180px] overflow-y-auto">
                      {storySituations.map((situation, index) => (
                        <div
                          key={situation.id}
                          className={cn(
                            "flex items-center gap-2 p-1.5 rounded text-[10px]",
                            situation.status === 'done' && "bg-emerald-500/10 border border-emerald-500/20",
                            situation.status === 'generating' && "bg-violet-500/10 border border-violet-500/20",
                            situation.status === 'pending' && "bg-muted/30",
                            situation.status === 'error' && "bg-red-500/10 border border-red-500/20"
                          )}
                        >
                          {situation.status === 'done' && (
                            <CheckCircle className="h-3.5 w-3.5 text-emerald-500 shrink-0" />
                          )}
                          {situation.status === 'generating' && (
                            <Loader2 className="h-3.5 w-3.5 text-violet-500 animate-spin shrink-0" />
                          )}
                          {situation.status === 'pending' && (
                            <div className="h-3.5 w-3.5 rounded-full border border-muted-foreground/30 shrink-0" />
                          )}
                          {situation.status === 'error' && (
                            <AlertCircle className="h-3.5 w-3.5 text-red-500 shrink-0" />
                          )}
                          <span className="font-medium text-muted-foreground">{index + 1}.</span>
                          <span className="truncate flex-1">{situation.text}</span>
                          {situation.status === 'done' && (
                            <Button
                              variant="ghost"
                              size="icon"
                              onClick={() => regenerateParagraph(index)}
                              disabled={isGenerating}
                              className="h-5 w-5 shrink-0 opacity-50 hover:opacity-100"
                            >
                              <RefreshCw className="h-3 w-3" />
                            </Button>
                          )}
                        </div>
                      ))}
                    </div>

                    {/* Done - Show insert button */}
                    {currentGeneratingIndex === -1 && !isGenerating && storySituations.some(s => s.status === 'done') && (
                      <div className="flex gap-2">
                        <Button
                          onClick={resetStory}
                          variant="outline"
                          className="flex-1 h-8 text-xs"
                        >
                          New Story
                        </Button>
                        <Button
                          onClick={insertStoryToEditor}
                          className="flex-1 h-8 text-xs bg-gradient-to-r from-violet-500 to-fuchsia-500 border-0"
                        >
                          <ArrowRight className="h-3 w-3 mr-1" />
                          Insert to Editor
                        </Button>
                      </div>
                    )}
                  </>
                )}
              </div>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </div>
  )
}
