// Quote list and formatting adapted from Illuminate/Foundation/Inspiring.php.
// Laravel Framework v13.40.0, commit 1449bf241017087fbd38c118ecaabef3f6797fdc.
/*
The MIT License (MIT)

Copyright (c) Taylor Otwell

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
*/

const QUOTES: [&str; 41] = [
    "Act only according to that maxim whereby you can, at the same time, will that it should become a universal law. - Immanuel Kant",
    "An unexamined life is not worth living. - Socrates",
    "Be present above all else. - Naval Ravikant",
    "Do what you can, with what you have, where you are. - Theodore Roosevelt",
    "Happiness is not something readymade. It comes from your own actions. - Dalai Lama",
    "He who is contented is rich. - Laozi",
    "I begin to speak only when I am certain what I will say is not better left unsaid. - Cato the Younger",
    "I have not failed. I've just found 10,000 ways that won't work. - Thomas Edison",
    "If you do not have a consistent goal in life, you can not live it in a consistent way. - Marcus Aurelius",
    "It is never too late to be what you might have been. - George Eliot",
    "It is not the man who has too little, but the man who craves more, that is poor. - Seneca",
    "It is quality rather than quantity that matters. - Lucius Annaeus Seneca",
    "Knowing is not enough; we must apply. Being willing is not enough; we must do. - Leonardo da Vinci",
    "Let all your things have their places; let each part of your business have its time. - Benjamin Franklin",
    "Live as if you were to die tomorrow. Learn as if you were to live forever. - Mahatma Gandhi",
    "No surplus words or unnecessary actions. - Marcus Aurelius",
    "Nothing worth having comes easy. - Theodore Roosevelt",
    "Order your soul. Reduce your wants. - Augustine",
    "People find pleasure in different ways. I find it in keeping my mind clear. - Marcus Aurelius",
    "Simplicity is an acquired taste. - Katharine Gerould",
    "Simplicity is the consequence of refined emotions. - Jean D'Alembert",
    "Simplicity is the essence of happiness. - Cedric Bledsoe",
    "Simplicity is the ultimate sophistication. - Leonardo da Vinci",
    "Smile, breathe, and go slowly. - Thich Nhat Hanh",
    "The only way to do great work is to love what you do. - Steve Jobs",
    "The whole future lies in uncertainty: live immediately. - Seneca",
    "Very little is needed to make a happy life. - Marcus Aurelius",
    "Waste no more time arguing what a good man should be, be one. - Marcus Aurelius",
    "Well begun is half done. - Aristotle",
    "When there is no desire, all things are at peace. - Laozi",
    "Walk as if you are kissing the Earth with your feet. - Thich Nhat Hanh",
    "Because you are alive, everything is possible. - Thich Nhat Hanh",
    "Breathing in, I calm body and mind. Breathing out, I smile. - Thich Nhat Hanh",
    "Life is available only in the present moment. - Thich Nhat Hanh",
    "The best way to take care of the future is to take care of the present moment. - Thich Nhat Hanh",
    "Nothing in life is to be feared, it is only to be understood. Now is the time to understand more, so that we may fear less. - Maria Skłodowska-Curie",
    "The biggest battle is the war against ignorance. - Mustafa Kemal Atatürk",
    "Always remember that you are absolutely unique. Just like everyone else. - Margaret Mead",
    "You must be the change you wish to see in the world. - Mahatma Gandhi",
    "It always seems impossible until it is done. - Nelson Mandela",
    "We must ship. - Taylor Otwell",
];

/// Selects and formats an inspirational quote for the console.
pub struct Inspiring;

impl Inspiring {
    pub fn quote() -> String {
        let quote = Self::quotes()[fastrand::usize(..QUOTES.len())];
        Self::formatForConsole(quote)
    }

    pub fn quotes() -> &'static [&'static str] {
        &QUOTES
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Inspiring formatter name")]
    fn formatForConsole(quote: &str) -> String {
        // Laravel takes the first two fields from explode('-').
        let mut fields = quote.split('-').map(str::trim);
        let text = fields.next().expect("quote text");
        let author = fields.next().expect("quote author");
        format!("\n  <options=bold>“ {text} ”</>\n  <fg=gray>— {author}</>\n")
    }
}
