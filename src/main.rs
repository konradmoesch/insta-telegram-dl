use std::num::ParseIntError;
use std::time::Duration;

use instagram_scraper_rs::{InstagramScraper, InstagramScraperResult, Post};
use serde_derive::{Deserialize, Serialize};
use telegram_bot2::{Bot, bot, BotBuilder, Builder, command, commands, handler, handlers};
use telegram_bot2::log::info;
use telegram_bot2::models::{ChatId, GetChatBuilder, Message, SendMessageBuilder};

#[derive(Debug, PartialEq)]
enum UserState {
    Allowed,
    Admin,
    NotAllowed,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
struct MyConfig {
    version: u8,
    tg_api_key: String,
    admin_user: ChatId,
    allowed_users: Vec<ChatId>,
}

async fn scrape_instagram(username_to_scrape: &str, number_to_scrape: usize) -> Option<Vec<Post>> {
    let username = std::env::var("INSTAGRAM_USERNAME").ok();
    let password = std::env::var("INSTAGRAM_PASSWORD").ok();
    let mut scraper = InstagramScraper::default();
    if let (Some(username), Some(password)) = (username, password) {
        println!("authenticating with username {}", username);
        scraper = scraper.authenticate_with_login(username, password);
    }
    scraper.login().await.unwrap();

    let user = scraper.scrape_userinfo(username_to_scrape).await;
    // collect user's stories and up to 10 highlighted stories
    //let stories = scraper.scrape_user_stories(&user.id, 10).await.unwrap();
    // collect last 10 posts
    match user {
        Ok(u) => {
            let posts = scraper.scrape_posts(&u.id, number_to_scrape).await.unwrap();
            Some(posts)
        }
        Err(_) => { None }
    }
}

#[bot]
async fn bot() -> _ {
    pretty_env_logger::init();
    info!("logger inited");
    let mut cfg: MyConfig = confy::load("insta-telegram-dl", None).unwrap();
    dbg!(cfg.clone());
    dbg!(confy::get_configuration_file_path("insta-telegram-dl", None).unwrap());
    confy::store("insta-telegram-dl", None, cfg.clone()).unwrap();

    BotBuilder::new()
        .interval(Duration::from_secs(0))
        .timeout(5)
        .handlers(handlers![handler])
        .commands(commands![status, request_access, allow])
}

fn get_user_state(chat_id: &ChatId, current_config: &MyConfig) -> UserState {
    let mut user_state = UserState::NotAllowed;
    if current_config.allowed_users.contains(&chat_id) { user_state = UserState::Allowed };
    if current_config.admin_user == *chat_id { user_state = UserState::Admin };
    user_state
}

#[handler]
async fn handler(message: &Message, bot: &Bot) -> Result<(), ()> {
    info!("message received: {:?}", message);
    let current_config: MyConfig = confy::load("insta-telegram-dl", None).unwrap();
    let chat_id = ChatId::from(message.chat.id);
    let user_state = get_user_state(&chat_id, &current_config);
    if user_state == UserState::NotAllowed {
        let error_response = "You are not allowed to use this bot. Please /request_access to continue.";
        bot.send_message(SendMessageBuilder::new(chat_id, error_response.to_string()).build()).await.unwrap();
    } else {
        let incoming_text = message.text.clone().unwrap_or_default();
        let mut incoming_words = incoming_text.split_whitespace();
        if incoming_words.clone().count() > 2 {
            bot.send_message(SendMessageBuilder::new(chat_id.clone(), "invalid number of arguments: usage: [username] ([number_to_scrape]=10)".to_string()).build()).await.unwrap();
        } else {
            let username_to_scrape = incoming_words.next().unwrap();
            let count_to_scrape = match incoming_words.next().unwrap_or("10").parse::<usize>() {
                Ok(c) => {c}
                Err(_) => {
                    bot.send_message(SendMessageBuilder::new(chat_id.clone(), "invalid number_to_scrape, using default (10)".to_string()).build()).await.unwrap();
                    10
                }
            };
            let posts_option = scrape_instagram(username_to_scrape, count_to_scrape).await;
            match posts_option {
                None => {
                    bot.send_message(SendMessageBuilder::new(chat_id.clone(), "User not found".to_string()).build()).await.unwrap();
                }
                Some(posts) => {
                    for post in posts {
                        bot.send_message(SendMessageBuilder::new(chat_id.clone(), post.display_url).build()).await.unwrap();
                    }
                }
            }
        }
    }
    Ok(())
}

#[command("/request_access")]
async fn request_access(bot: &Bot, chat_id: ChatId) -> Result<(), ()> {
    let current_config: MyConfig = confy::load("insta-telegram-dl", None).unwrap();
    let user_id = match &chat_id {
        ChatId::Integer(userid) => *userid,
        ChatId::String(_) => 0,
    };
    let chat = bot.get_chat(GetChatBuilder::new(chat_id.clone()).build()).await.unwrap();

    bot.send_message(SendMessageBuilder::new(current_config.admin_user, format!("User {} {} ({}) [{:?}] wants to get access", chat.first_name.unwrap_or_default(), chat.last_name.unwrap_or_default(), chat.username.unwrap_or_default(), chat_id.clone())).build()).await.unwrap();
    bot.send_message(SendMessageBuilder::new(chat_id.clone(), format!("You are user {:?}, request has been submitted", chat_id)).build()).await.unwrap();
    Ok(())
}

#[command("/allow <id_to_be_allowed>")]
async fn allow(bot: &Bot, chat_id: ChatId, id_to_be_allowed: i64) -> Result<(), ()> {
    let mut current_config: MyConfig = confy::load("insta-telegram-dl", None).unwrap();
    let user_state = get_user_state(&chat_id, &current_config);
    match user_state {
        UserState::Admin => {
            match bot.get_chat(GetChatBuilder::new(ChatId::from(id_to_be_allowed)).build()).await {
                Ok(_) => {
                    if !current_config.allowed_users.contains(&ChatId::from(id_to_be_allowed)) {
                        current_config.allowed_users.push(ChatId::from(id_to_be_allowed));
                    }
                    confy::store("insta-telegram-dl", None, current_config.clone()).unwrap();
                    bot.send_message(SendMessageBuilder::new(current_config.admin_user, format!("User {} added to the allowlist", id_to_be_allowed)).build()).await.unwrap();
                    bot.send_message(SendMessageBuilder::new(ChatId::from(id_to_be_allowed), format!("You are now allowed. Have fun!🎉")).build()).await.unwrap();
                }
                Err(e) => {
                    bot.send_message(SendMessageBuilder::new(current_config.admin_user, format!("An error occurred trying to add user {} to the allowlist: {:?}", id_to_be_allowed, e)).build()).await.unwrap();
                }
            }
        }
        _ => {
            let error_response = "You are not allowed to use this command.";
            bot.send_message(SendMessageBuilder::new(chat_id, error_response.to_string()).build()).await.unwrap();
        }
    }
    Ok(())
}

#[command("/status")]
async fn status(bot: &Bot, chat: ChatId) -> Result<(), ()> {
    let mut current_config: MyConfig = confy::load("insta-telegram-dl", None).unwrap();
    let user_state = get_user_state(&chat, &current_config);
    current_config.allowed_users.push(chat.clone());
    confy::store("insta-telegram-dl", None, current_config).unwrap();
    bot.send_message(SendMessageBuilder::new(chat.clone(), format!("You are user {:?}, your current state is {:?}", chat.clone(), user_state)).build()).await.unwrap();
    Ok(())
}
