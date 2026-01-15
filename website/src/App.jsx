import { useState, useEffect } from 'react'
import { Zap, Shield, Globe, Search } from 'lucide-react'

function App() {
  const [scrollProgress, setScrollProgress] = useState(0)
  
  useEffect(() => {
    const handleScroll = () => {
      const windowHeight = window.innerHeight
      const documentHeight = document.documentElement.scrollHeight
      const scrollTop = window.scrollY
      const progress = (scrollTop / (documentHeight - windowHeight)) * 100
      setScrollProgress(progress)
    }
    window.addEventListener('scroll', handleScroll)
    return () => window.removeEventListener('scroll', handleScroll)
  }, [])

  return (
    <div className="min-h-screen bg-black text-white">
      <div className="fixed top-0 left-0 right-0 h-1 bg-gray-800 z-50">
        <div 
          className="h-full bg-gradient-to-r from-blue-500 to-purple-500 transition-all duration-150"
          style={{ width: `${scrollProgress}%` }}
        />
      </div>
      
      {/* Hero Section */}
      <section className="relative min-h-screen flex items-center justify-center overflow-hidden">
        <div className="absolute inset-0 bg-gradient-to-b from-blue-900/20 to-black" />
        <div className="relative z-10 text-center px-4">
          <h1 className="text-6xl md:text-8xl font-bold mb-6 animate-pulse">
            Aegistry
          </h1>
          <p className="text-xl md:text-2xl mb-8 text-gray-300">
            Low-latency sanctions & PEP screening API
          </p>
          <div className="flex gap-4 justify-center">
            <button className="px-8 py-4 bg-blue-600 rounded-lg font-semibold hover:bg-blue-700 transition transform hover:scale-105">
              Get Started
            </button>
            <button className="px-8 py-4 border border-gray-600 rounded-lg font-semibold hover:bg-gray-800 transition transform hover:scale-105">
              View Docs
            </button>
          </div>
        </div>
      </section>

      {/* Features Section */}
      <section className="py-32 px-4">
        <div className="max-w-6xl mx-auto">
          <h2 className="text-4xl font-bold text-center mb-16">Features</h2>
          <div className="grid md:grid-cols-3 gap-8">
            <div className="p-6 bg-gray-900 rounded-lg border border-gray-800 hover:border-blue-500 transition">
              <Zap className="w-12 h-12 mb-4 text-blue-500" />
              <h3 className="text-xl font-semibold mb-2">Ultra Fast</h3>
              <p className="text-gray-400">Sub-10ms p95 latency</p>
            </div>
            <div className="p-6 bg-gray-900 rounded-lg border border-gray-800 hover:border-green-500 transition">
              <Shield className="w-12 h-12 mb-4 text-green-500" />
              <h3 className="text-xl font-semibold mb-2">EU Privacy</h3>
              <p className="text-gray-400">GDPR compliant</p>
            </div>
            <div className="p-6 bg-gray-900 rounded-lg border border-gray-800 hover:border-purple-500 transition">
              <Globe className="w-12 h-12 mb-4 text-purple-500" />
              <h3 className="text-xl font-semibold mb-2">Global Coverage</h3>
              <p className="text-gray-400">All major sanctions lists</p>
            </div>
          </div>
        </div>
      </section>

      {/* Stats Section */}
      <section className="py-32 px-4 bg-gray-900">
        <div className="max-w-6xl mx-auto text-center">
          <h2 className="text-4xl font-bold mb-16">Performance</h2>
          <div className="grid md:grid-cols-3 gap-8">
            <div>
              <div className="text-5xl font-bold text-blue-500 mb-2">10ms</div>
              <p className="text-gray-400">p95 Latency</p>
            </div>
            <div>
              <div className="text-5xl font-bold text-green-500 mb-2">99.9%</div>
              <p className="text-gray-400">Uptime</p>
            </div>
            <div>
              <div className="text-5xl font-bold text-purple-500 mb-2">1M+</div>
              <p className="text-gray-400">Records</p>
            </div>
          </div>
        </div>
      </section>

      {/* CTA Section */}
      <section className="py-32 px-4">
        <div className="max-w-4xl mx-auto text-center">
          <div className="p-8 bg-gradient-to-r from-blue-900/50 to-purple-900/50 rounded-lg">
            <h2 className="text-4xl font-bold mb-8">Ready to get started?</h2>
            <button className="px-12 py-6 bg-gradient-to-r from-blue-600 to-purple-600 rounded-lg font-semibold text-xl hover:from-blue-700 hover:to-purple-700 transition transform hover:scale-105">
              Start Screening Now
            </button>
          </div>
        </div>
      </section>
    </div>
  )
}

export default App

