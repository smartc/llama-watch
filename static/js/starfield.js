/**
 * Starfield Background Component
 * A vanilla JavaScript animated starfield background for headers
 * Converted from the AstroCat Vue.js component
 */

class StarfieldBackground {
    constructor(container, options = {}) {
        this.container = container;
        this.options = {
            numStars: options.numStars || 55,
            minSize: options.minSize || 0.4,
            maxSize: options.maxSize || 1.0,
            gradient: options.gradient || 'linear-gradient(135deg, #070F12 0%, #0C0F1A 100%)'
        };
        this.stars = [];

        this.init();
    }

    init() {
        // Create container structure
        this.container.innerHTML = `
            <div class="starfield-container">
                <div class="starfield-gradient" style="background: ${this.options.gradient}"></div>
                <svg class="starfield-svg" xmlns="http://www.w3.org/2000/svg" preserveAspectRatio="none"></svg>
            </div>
        `;

        this.svg = this.container.querySelector('.starfield-svg');
        this.generateStars();
        this.renderStars();
    }

    generateStars() {
        // Color palette with weights (more white stars, fewer colored)
        const palette = [
            { color: 'white', weight: 55 },
            { color: '#C8DCFF', weight: 25 }, // cool blue
            { color: '#FFB57A', weight: 15 }, // warm orange
            { color: 'white', weight: 5 }
        ];

        // Build weighted color array
        const weightedColors = palette.flatMap(c =>
            Array(c.weight).fill(c.color)
        );

        // Generate random stars
        this.stars = Array.from({ length: this.options.numStars }, () => {
            return {
                cx: Math.random() * 100,
                cy: Math.random() * 100,
                r: this.options.minSize + Math.random() * (this.options.maxSize - this.options.minSize),
                opacity: 0.2 + Math.random() * 0.3,
                color: weightedColors[Math.floor(Math.random() * weightedColors.length)]
            };
        });
    }

    renderStars() {
        // Calculate animation delays
        const delays = ['0s', '1s', '2s', '3s'];

        this.stars.forEach((star, index) => {
            const circle = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
            circle.setAttribute('cx', `${star.cx}%`);
            circle.setAttribute('cy', `${star.cy}%`);
            circle.setAttribute('r', star.r);
            circle.setAttribute('fill', star.color);
            circle.setAttribute('opacity', star.opacity);

            // Add animation
            const delay = delays[index % 4];
            circle.style.animation = `starTwinkle 4s infinite ease-in-out ${delay}`;

            this.svg.appendChild(circle);
        });
    }
}

// Initialize starfield when DOM is loaded
document.addEventListener('DOMContentLoaded', function() {
    // Create a container for the starfield and append it to body
    const starfieldWrapper = document.createElement('div');
    starfieldWrapper.id = 'starfield-wrapper';
    document.body.insertBefore(starfieldWrapper, document.body.firstChild);

    // Initialize starfield with more stars for full page coverage
    new StarfieldBackground(starfieldWrapper, {
        numStars: 150,  // More stars for full page
        minSize: 0.3,
        maxSize: 1.2
    });
});
